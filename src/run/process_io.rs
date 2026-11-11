use std::fs;
use std::io;
use std::path;
use std::process::{Command, Stdio};
use std::sync::mpsc;

#[cfg(unix)]
use std::os::unix;

use crate::error::AppError;
use crate::error::AppErrorKind;
use crate::error::AppResult;
use crate::types::event::AppEvent;
use crate::types::io::FileOperationProgress;
use crate::types::io::IoTaskStat;
use crate::types::io::{FileOperation, FileOperationOptions, IoTask, IoTaskProgressMessage};
use crate::utils::fs::query_number_of_items;
use crate::utils::name_resolution::rename_filename_conflict;

/// Runs on the dedicated IO worker thread: processes queued [`IoTask`]s one at a time,
/// reporting each result back over `event_tx`.
pub fn process_io_tasks(
    event_rx: mpsc::Receiver<IoTask>,
    event_tx: mpsc::Sender<AppEvent>,
) -> AppResult {
    while let Ok(io_task) = event_rx.recv() {
        let res = process_io_task(&io_task, &event_tx);
        let event = AppEvent::IoTaskResult(res);
        let _ = event_tx.send(event);
    }
    Ok(())
}

/// Runs a single [`IoTask`] to completion, reporting start progress before dispatching to the
/// operation-specific implementation (cut/copy/delete/symlink).
pub fn process_io_task(io_task: &IoTask, event_tx: &mpsc::Sender<AppEvent>) -> AppResult {
    if matches!(
        io_task.get_operation_type(),
        FileOperation::Cut | FileOperation::Copy
    ) {
        check_dest_not_inside_src(io_task)?;
    }

    // Sources can vanish between staging and pasting (deleted or renamed externally): skip
    // them instead of aborting the whole task, so the remaining files are still processed.
    let mut paths: Vec<path::PathBuf> = Vec::with_capacity(io_task.paths.len());
    for path in io_task.paths.iter() {
        if path.symlink_metadata().is_ok() {
            paths.push(path.clone());
        } else {
            let _ = event_tx.send(AppEvent::IoTaskProgress(
                IoTaskProgressMessage::SourceMissing {
                    file_path: path.clone(),
                },
            ));
        }
    }
    if paths.is_empty() {
        let msg = if io_task.paths.is_empty() {
            "no source files to process".to_string()
        } else {
            "all source files are missing".to_string()
        };
        return Err(AppError::new(AppErrorKind::InvalidParameters, msg));
    }

    let (total_files, total_bytes) = query_number_of_items(paths.as_slice())?;
    let src = paths
        .first()
        .and_then(|p| p.parent())
        .unwrap_or(io_task.dest.as_path())
        .to_path_buf();
    let dest = io_task.dest.clone();

    let operation_progress = FileOperationProgress {
        kind: io_task.operation,
        current_file: paths.first().cloned().unwrap_or_default(),
        total_files,
        files_processed: 0,
        total_bytes,
        bytes_processed: 0,
    };

    let io_stat = IoTaskStat::new(operation_progress, src, dest);
    let event = AppEvent::IoTaskStart(io_stat);
    let _ = event_tx.send(event);

    let res = match io_task.get_operation_type() {
        FileOperation::Cut => paste_cut(&paths, io_task, event_tx),
        FileOperation::Copy => paste_copy(&paths, io_task, event_tx),
        FileOperation::Delete => delete(&paths, io_task, event_tx),
        FileOperation::Symlink => paste_symlink(&paths, io_task, event_tx),
    };
    res?;
    Ok(())
}

/// Errors if the destination is one of the source directories or inside one,
/// which would otherwise recurse forever.
fn check_dest_not_inside_src(io_task: &IoTask) -> AppResult {
    let dest = io_task
        .dest
        .canonicalize()
        .unwrap_or_else(|_| io_task.dest.clone());
    for path in io_task.paths.iter() {
        let is_dir = path.symlink_metadata().map(|m| m.is_dir()).unwrap_or(false);
        if !is_dir {
            continue;
        }
        let src = path.canonicalize().unwrap_or_else(|_| path.clone());
        if dest.starts_with(&src) {
            return Err(AppError::new(
                AppErrorKind::InvalidParameters,
                format!("Cannot paste '{}' into itself", path.to_string_lossy()),
            ));
        }
    }
    Ok(())
}

fn paste_copy(paths: &[path::PathBuf], task: &IoTask, tx: &mpsc::Sender<AppEvent>) -> AppResult {
    for path in paths {
        if path.symlink_metadata().is_err() {
            continue;
        }
        recursive_copy(tx, path.as_path(), task.dest.as_path(), task.options)?;
    }
    Ok(())
}

fn paste_cut(paths: &[path::PathBuf], task: &IoTask, tx: &mpsc::Sender<AppEvent>) -> AppResult {
    for path in paths {
        if path.symlink_metadata().is_err() {
            continue;
        }
        recursive_cut(tx, path.as_path(), task.dest.as_path(), task.options)?;
    }
    Ok(())
}

fn paste_symlink(paths: &[path::PathBuf], task: &IoTask, tx: &mpsc::Sender<AppEvent>) -> AppResult {
    match task.options.symlink_relative {
        true => paste_link_relative(paths, task, tx),
        false => paste_link_absolute(paths, task, tx),
    }
}

fn paste_link_absolute(
    paths: &[path::PathBuf],
    task: &IoTask,
    tx: &mpsc::Sender<AppEvent>,
) -> AppResult {
    #[cfg(unix)]
    for src in paths {
        let event = IoTaskProgressMessage::FileStart {
            file_path: src.to_path_buf(),
        };
        let _ = tx.send(AppEvent::IoTaskProgress(event));
        let mut dest_buf = task.dest.to_path_buf();
        if let Some(s) = src.file_name() {
            dest_buf.push(s);
        }
        if !task.options.overwrite {
            rename_filename_conflict(&mut dest_buf);
        }
        unix::fs::symlink(src, &dest_buf)?;
        let event = IoTaskProgressMessage::FileComplete { file_size: 1 };
        let _ = tx.send(AppEvent::IoTaskProgress(event));
    }
    Ok(())
}

fn paste_link_relative(
    paths: &[path::PathBuf],
    task: &IoTask,
    tx: &mpsc::Sender<AppEvent>,
) -> AppResult {
    #[cfg(unix)]
    for src in paths {
        let event = IoTaskProgressMessage::FileStart {
            file_path: src.to_path_buf(),
        };
        let _ = tx.send(AppEvent::IoTaskProgress(event));
        let mut dest_buf = task.dest.to_path_buf();
        if let Some(s) = src.file_name() {
            dest_buf.push(s);
        }
        if !task.options.overwrite {
            rename_filename_conflict(&mut dest_buf);
        }
        let mut src_components = src.components();
        let mut dest_components = dest_buf.components();

        // skip to where the two paths diverge
        let mut non_relative_path = path::PathBuf::new();
        for (s, d) in src_components.by_ref().zip(dest_components.by_ref()) {
            if s != d {
                non_relative_path.push(s);
                break;
            }
        }

        let mut relative_path = path::PathBuf::new();
        for _ in dest_components {
            relative_path.push("..");
        }
        relative_path.push(non_relative_path);
        for s in src_components {
            relative_path.push(s);
        }
        unix::fs::symlink(relative_path, &dest_buf)?;

        let event = IoTaskProgressMessage::FileComplete { file_size: 1 };
        let _ = tx.send(AppEvent::IoTaskProgress(event));
    }
    Ok(())
}

fn delete(paths: &[path::PathBuf], task: &IoTask, tx: &mpsc::Sender<AppEvent>) -> AppResult {
    if task.options.permanently {
        remove_files(paths, tx)?;
    } else {
        trash_files(paths, tx)?;
    }
    Ok(())
}

/// Recursively copies `src` into `dest`; on conflict, skips if `options.skip_exist`, overwrites
/// if `options.overwrite`, otherwise renames. Reports per-file progress over `tx`.
pub fn recursive_copy(
    tx: &mpsc::Sender<AppEvent>,
    src: &path::Path,
    dest: &path::Path,
    options: FileOperationOptions,
) -> io::Result<()> {
    let event = IoTaskProgressMessage::FileStart {
        file_path: src.to_path_buf(),
    };
    let _ = tx.send(AppEvent::IoTaskProgress(event));

    let mut dest_buf = dest.to_path_buf();
    if let Some(s) = src.file_name() {
        dest_buf.push(s);
    }

    let file_type = fs::symlink_metadata(src)?.file_type();
    if file_type.is_dir() {
        match fs::create_dir(dest_buf.as_path()) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                // An existing directory is merged into when overwriting or when skipping
                // existing files (`cp -n` semantics: only missing children are copied);
                // otherwise the destination is renamed out of the way, as before.
                if !options.overwrite && !options.skip_exist {
                    rename_filename_conflict(&mut dest_buf);
                    fs::create_dir(dest_buf.as_path())?;
                }
            }
            e => return e,
        }
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let entry_path = entry.path();
            recursive_copy(tx, entry_path.as_path(), dest_buf.as_path(), options)?;
        }
        let event = IoTaskProgressMessage::FileComplete { file_size: 1 };
        let _ = tx.send(AppEvent::IoTaskProgress(event));

        Ok(())
    } else if file_type.is_file() {
        if options.skip_exist && dest_buf.symlink_metadata().is_ok() {
            // "skip existing": leave the destination as-is, count the file towards progress
            let event = IoTaskProgressMessage::FileComplete { file_size: 0 };
            let _ = tx.send(AppEvent::IoTaskProgress(event));
            return Ok(());
        }
        if !options.overwrite {
            rename_filename_conflict(&mut dest_buf);
        }
        let bytes_processed = fs::copy(src, dest_buf)?;
        let event = IoTaskProgressMessage::FileComplete {
            file_size: bytes_processed,
        };
        let _ = tx.send(AppEvent::IoTaskProgress(event));

        Ok(())
    } else if file_type.is_symlink() {
        if options.skip_exist && dest_buf.symlink_metadata().is_ok() {
            // "skip existing": leave the destination as-is, count the file towards progress
            let event = IoTaskProgressMessage::FileComplete { file_size: 0 };
            let _ = tx.send(AppEvent::IoTaskProgress(event));
            return Ok(());
        }
        if !options.overwrite {
            rename_filename_conflict(&mut dest_buf);
        }
        let link_path = fs::read_link(src)?;
        std::os::unix::fs::symlink(link_path, dest_buf)?;
        let event = IoTaskProgressMessage::FileComplete { file_size: 1 };
        let _ = tx.send(AppEvent::IoTaskProgress(event));

        Ok(())
    } else {
        Ok(())
    }
}

/// Recursively moves `src` into `dest`, falling back to copy-then-delete across filesystems;
/// on conflict, skips if `options.skip_exist`, overwrites if `options.overwrite`, otherwise
/// renames. Reports per-file progress over `tx`.
pub fn recursive_cut(
    tx: &mpsc::Sender<AppEvent>,
    src: &path::Path,
    dest: &path::Path,
    options: FileOperationOptions,
) -> io::Result<()> {
    let event = IoTaskProgressMessage::FileStart {
        file_path: src.to_path_buf(),
    };
    let _ = tx.send(AppEvent::IoTaskProgress(event));

    let mut dest_buf = dest.to_path_buf();
    if let Some(s) = src.file_name() {
        dest_buf.push(s);
    }
    if options.skip_exist && dest_buf.symlink_metadata().is_ok() {
        // "skip existing": leave the destination as-is and keep the source untouched
        let event = IoTaskProgressMessage::FileComplete { file_size: 0 };
        let _ = tx.send(AppEvent::IoTaskProgress(event));
        return Ok(());
    }
    if !options.overwrite {
        rename_filename_conflict(&mut dest_buf);
    }

    let metadata = fs::symlink_metadata(src)?;
    let file_type = metadata.file_type();

    match fs::rename(src, dest_buf.as_path()) {
        Ok(_) => {
            let bytes_processed = metadata.len();
            let event = IoTaskProgressMessage::FileComplete {
                file_size: bytes_processed,
            };
            let _ = tx.send(AppEvent::IoTaskProgress(event));
        }
        // rename(2) can't move across filesystems, or merge into an existing directory
        Err(err)
            if err.kind() == io::ErrorKind::CrossesDevices
                || (options.overwrite && file_type.is_dir() && dest_buf.is_dir()) =>
        {
            if file_type.is_dir() {
                match fs::create_dir(dest_buf.as_path()) {
                    Err(e) if !(options.overwrite && e.kind() == io::ErrorKind::AlreadyExists) => {
                        return Err(e)
                    }
                    _ => {}
                }
                for entry in fs::read_dir(src)? {
                    let entry_path = entry?.path();
                    recursive_cut(tx, entry_path.as_path(), dest_buf.as_path(), options)?;
                }
                fs::remove_dir(src)?;
                let event = IoTaskProgressMessage::FileComplete { file_size: 1 };
                let _ = tx.send(AppEvent::IoTaskProgress(event));
            } else if file_type.is_symlink() {
                let link_path = fs::read_link(src)?;
                std::os::unix::fs::symlink(link_path, dest_buf)?;
                fs::remove_file(src)?;

                let bytes_processed = metadata.len();
                let event = IoTaskProgressMessage::FileComplete {
                    file_size: bytes_processed,
                };
                let _ = tx.send(AppEvent::IoTaskProgress(event));
            } else {
                let bytes_processed = fs::copy(src, dest_buf.as_path())?;
                fs::remove_file(src)?;

                let event = IoTaskProgressMessage::FileComplete {
                    file_size: bytes_processed,
                };
                let _ = tx.send(AppEvent::IoTaskProgress(event));
            }
        }
        Err(err) => return Err(err),
    }
    Ok(())
}

fn remove_files<P>(paths: &[P], tx: &mpsc::Sender<AppEvent>) -> std::io::Result<()>
where
    P: AsRef<path::Path>,
{
    for path in paths {
        if let Ok(metadata) = fs::symlink_metadata(path) {
            let event = IoTaskProgressMessage::FileStart {
                file_path: path.as_ref().to_path_buf(),
            };
            let _ = tx.send(AppEvent::IoTaskProgress(event));

            if metadata.is_dir() {
                fs::remove_dir_all(path)?;
            } else {
                fs::remove_file(path)?;
            }
            let bytes_processed = metadata.len();
            let event = IoTaskProgressMessage::FileComplete {
                file_size: bytes_processed,
            };
            let _ = tx.send(AppEvent::IoTaskProgress(event));
        }
    }
    Ok(())
}

fn trash_files<P>(paths: &[P], tx: &mpsc::Sender<AppEvent>) -> AppResult
where
    P: AsRef<path::Path>,
{
    for path in paths {
        let event = IoTaskProgressMessage::FileStart {
            file_path: path.as_ref().to_path_buf(),
        };
        let _ = tx.send(AppEvent::IoTaskProgress(event));

        trash_file(path)?;
        let event = IoTaskProgressMessage::FileComplete { file_size: 1 };
        let _ = tx.send(AppEvent::IoTaskProgress(event));
    }
    Ok(())
}

fn trash_file<P>(file_path: P) -> AppResult
where
    P: AsRef<path::Path>,
{
    let file_path = file_path.as_ref();
    let trash_commands: [&[&str]; 4] = [
        &["gio", "trash", "--"],
        &["trash-put", "--"],
        &["trash", "--"],
        &["gtrash", "put", "--"],
    ];

    for cmd in trash_commands {
        let status = Command::new(cmd[0])
            .args(&cmd[1..])
            .arg(file_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        if matches!(status, Ok(s) if s.success()) {
            return Ok(());
        }
    }
    Err(AppError::new(
        AppErrorKind::Trash,
        "Failed to trash file".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(op: FileOperation, src: &path::Path, dest: &path::Path) -> IoTask {
        IoTask::new(
            op,
            vec![src.to_path_buf()],
            dest.to_path_buf(),
            FileOperationOptions::default(),
        )
    }

    #[test]
    fn paste_into_itself_is_rejected() {
        let dir = std::env::temp_dir().join(format!("joshuto-test-{}", uuid::Uuid::new_v4()));
        let src = dir.join("a");
        fs::create_dir_all(src.join("b")).unwrap();

        let into_self = check_dest_not_inside_src(&task(FileOperation::Copy, &src, &src));
        let into_child = check_dest_not_inside_src(&task(FileOperation::Cut, &src, &src.join("b")));
        let into_parent = check_dest_not_inside_src(&task(FileOperation::Copy, &src, &dir));
        fs::remove_dir_all(&dir).unwrap();

        assert!(into_self.is_err());
        assert!(into_child.is_err());
        assert!(into_parent.is_ok());
    }

    #[test]
    fn cut_with_overwrite_merges_into_existing_dir() {
        let dir = std::env::temp_dir().join(format!("joshuto-test-{}", uuid::Uuid::new_v4()));
        let (src, dest) = (dir.join("src/a"), dir.join("dest"));
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(dest.join("a")).unwrap();
        fs::write(src.join("new"), b"new").unwrap();
        fs::write(dest.join("a/old"), b"old").unwrap();

        let (tx, _rx) = mpsc::channel();
        let options = FileOperationOptions {
            overwrite: true,
            ..Default::default()
        };
        let res = recursive_cut(&tx, &src, &dest, options);
        let (has_new, has_old, src_gone) = (
            dest.join("a/new").exists(),
            dest.join("a/old").exists(),
            !src.exists(),
        );
        fs::remove_dir_all(&dir).unwrap();

        res.unwrap();
        assert!(has_new && has_old && src_gone);
    }

    #[test]
    fn copy_dir_onto_existing_name_without_overwrite_renames() {
        let dir = std::env::temp_dir().join(format!("joshuto-test-{}", uuid::Uuid::new_v4()));
        let (src, dest) = (dir.join("src"), dir.join("dest"));
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(dest.join("src")).unwrap();
        fs::write(src.join("keep"), b"original").unwrap();
        fs::write(dest.join("src/other"), b"other").unwrap();

        let (tx, _rx) = mpsc::channel();
        recursive_copy(&tx, &src, &dest, FileOperationOptions::default()).unwrap();

        let (renamed, untouched) = (
            fs::read(dest.join("src_0/keep")).unwrap(),
            fs::read(dest.join("src/other")).unwrap(),
        );
        fs::remove_dir_all(&dir).unwrap();

        assert_eq!(renamed, b"original");
        assert_eq!(untouched, b"other");
    }

    #[test]
    fn copy_with_skip_exist_leaves_existing_files() {
        let dir = std::env::temp_dir().join(format!("joshuto-test-{}", uuid::Uuid::new_v4()));
        let (src, dest) = (dir.join("src"), dir.join("dest"));
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(src.join("keep"), b"original").unwrap();
        fs::write(src.join("fresh"), b"fresh").unwrap();
        fs::write(dest.join("keep"), b"existing").unwrap();
        // an existing subdirectory should be merged into, copying only missing children
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("sub/new"), b"new").unwrap();
        fs::create_dir_all(dest.join("sub")).unwrap();
        fs::write(dest.join("sub/old"), b"old").unwrap();

        let (tx, _rx) = mpsc::channel();
        let options = FileOperationOptions {
            skip_exist: true,
            ..Default::default()
        };
        // copy each child of `src` into `dest`, like `paste_files` does per staged entry
        for entry in fs::read_dir(&src).unwrap() {
            let entry = entry.unwrap();
            recursive_copy(&tx, &entry.path(), &dest, options).unwrap();
        }

        let (keep, fresh, sub_old, sub_new, src_intact) = (
            fs::read(dest.join("keep")).unwrap(),
            fs::read(dest.join("fresh")).unwrap(),
            fs::read(dest.join("sub/old")).unwrap(),
            fs::read(dest.join("sub/new")).unwrap(),
            fs::read(src.join("keep")).unwrap(),
        );
        fs::remove_dir_all(&dir).unwrap();

        assert_eq!(keep, b"existing");
        assert_eq!(fresh, b"fresh");
        assert_eq!(sub_old, b"old");
        assert_eq!(sub_new, b"new");
        assert_eq!(src_intact, b"original");
    }

    #[test]
    fn cut_with_skip_exist_keeps_source() {
        let dir = std::env::temp_dir().join(format!("joshuto-test-{}", uuid::Uuid::new_v4()));
        let (src, dest) = (dir.join("src"), dir.join("dest"));
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(dest.join("src")).unwrap();
        fs::write(src.join("keep"), b"original").unwrap();
        fs::write(dest.join("src/keep"), b"existing").unwrap();

        let (tx, _rx) = mpsc::channel();
        let options = FileOperationOptions {
            skip_exist: true,
            ..Default::default()
        };
        // moving the `src` directory into `dest` where `dest/src` already exists: skipped
        recursive_cut(&tx, &src, &dest, options).unwrap();

        let (keep, src_kept) = (
            fs::read(dest.join("src/keep")).unwrap(),
            src.join("keep").exists(),
        );
        fs::remove_dir_all(&dir).unwrap();

        assert_eq!(keep, b"existing");
        assert!(src_kept);
    }

    #[test]
    fn missing_sources_are_skipped_not_fatal() {
        let dir = std::env::temp_dir().join(format!("joshuto-test-{}", uuid::Uuid::new_v4()));
        let (src, dest) = (dir.join("src"), dir.join("dest"));
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(src.join("here"), b"data").unwrap();
        let gone = src.join("gone");

        let task = IoTask::new(
            FileOperation::Copy,
            vec![gone, src.join("here")],
            dest.clone(),
            FileOperationOptions::default(),
        );
        let (tx, rx) = mpsc::channel();
        let res = process_io_task(&task, &tx);
        // drain the progress events so the channel doesn't block
        drop(tx);
        for _ in rx {}
        let copied = fs::read(dest.join("here")).unwrap();
        fs::remove_dir_all(&dir).unwrap();

        res.unwrap();
        assert_eq!(copied, b"data");
    }
}
