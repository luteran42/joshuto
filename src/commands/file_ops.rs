use std::process::{Command, Stdio};

use crate::error::{AppError, AppErrorKind, AppResult};
use crate::fs::JoshutoDirList;
use crate::types::state::{AppState, LocalStateState};
use crate::workers::io::{FileOperation, FileOperationOptions, IoWorkerThread};

fn new_local_state(app_state: &mut AppState, file_op: FileOperation) -> Option<()> {
    let list = app_state
        .state
        .tab_state_ref()
        .curr_tab_ref()
        .curr_list_ref()?;
    let selected = list.get_selected_paths();

    let mut local_state = LocalStateState::new();
    local_state.set_paths(selected.into_iter());
    local_state.set_file_op(file_op);

    app_state.state.set_local_state(local_state);
    Some(())
}

fn mark_entries(app_state: &mut AppState, op: FileOperation) {
    let tab = app_state.state.tab_state_mut().curr_tab_mut();

    if let Some(curr_list) = tab.curr_list_mut() {
        curr_list.iter_mut().for_each(|entry| {
            entry.set_mark_cut_selected(false);
            entry.set_mark_copy_selected(false);
            entry.set_mark_sym_selected(false);
        });

        match curr_list.selected_count() {
            count if count != 0 => {
                curr_list.iter_mut().for_each(|entry| match op {
                    FileOperation::Cut if entry.is_permanent_selected() => {
                        entry.set_mark_cut_selected(true)
                    }
                    FileOperation::Copy if entry.is_permanent_selected() => {
                        entry.set_mark_copy_selected(true)
                    }
                    FileOperation::Symlink { .. } if entry.is_permanent_selected() => {
                        entry.set_mark_sym_selected(true)
                    }
                    _ => {}
                });
            }
            _ => {
                if let Some(entry) = curr_list.curr_entry_mut() {
                    match op {
                        FileOperation::Cut => entry.set_mark_cut_selected(true),
                        FileOperation::Copy => entry.set_mark_copy_selected(true),
                        FileOperation::Symlink { .. } => entry.set_mark_sym_selected(true),
                        _ => {}
                    }
                }
            }
        }
    }
}

fn unmark_entries(curr_tab: &mut JoshutoDirList) {
    if curr_tab.selected_count() != 0 {
        curr_tab.iter_mut().for_each(|entry| {
            if entry.is_marked_cut() {
                entry.set_mark_cut_selected(false)
            } else if entry.is_marked_copy() {
                entry.set_mark_copy_selected(false)
            } else if entry.is_marked_sym() {
                entry.set_mark_sym_selected(false)
            }
        })
    } else if let Some(entry) = curr_tab.curr_entry_mut() {
        if entry.is_marked_cut() {
            entry.set_mark_cut_selected(false)
        } else if entry.is_marked_copy() {
            entry.set_mark_copy_selected(false)
        } else if entry.is_marked_sym() {
            entry.set_mark_sym_selected(false)
        }
    }
}

fn unmark_and_cancel_all(app_state: &mut AppState) -> AppResult {
    app_state
        .state
        .tab_state_mut()
        .iter_mut()
        .for_each(|entry| {
            if let Some(curr_list) = entry.1.curr_list_mut() {
                unmark_entries(curr_list);
            }
            if let Some(par_list) = entry.1.parent_list_mut() {
                unmark_entries(par_list);
            }
            if let Some(child_list) = entry.1.child_list_mut() {
                unmark_entries(child_list);
            }
        });

    Err(AppError::new(
        AppErrorKind::Io,
        "File operation cancelled!".to_string(),
    ))
}

fn perform_file_operation(app_state: &mut AppState, op: FileOperation) -> AppResult {
    mark_entries(app_state, op);
    new_local_state(app_state, op);
    Ok(())
}

pub fn cut(app_state: &mut AppState) -> AppResult {
    perform_file_operation(app_state, FileOperation::Cut)?;
    Ok(())
}

pub fn copy(app_state: &mut AppState) -> AppResult {
    perform_file_operation(app_state, FileOperation::Copy)?;
    Ok(())
}

pub fn symlink_absolute(app_state: &mut AppState) -> AppResult {
    perform_file_operation(app_state, FileOperation::Symlink { relative: false })?;
    Ok(())
}

pub fn symlink_relative(app_state: &mut AppState) -> AppResult {
    perform_file_operation(app_state, FileOperation::Symlink { relative: true })?;
    Ok(())
}

pub fn paste(app_state: &mut AppState, options: FileOperationOptions) -> AppResult {
    match app_state.state.take_local_state() {
        Some(state) if !state.paths.is_empty() => {
            if options.cancel {
                unmark_and_cancel_all(app_state)?;
            }

            let dest = app_state
                .state
                .tab_state_ref()
                .curr_tab_ref()
                .get_cwd()
                .to_path_buf();
            let worker_thread = IoWorkerThread::new(state.file_op, state.paths, dest, options);
            app_state
                .state
                .worker_state_mut()
                .push_worker(worker_thread);
            Ok(())
        }
        _ => Err(AppError::new(
            AppErrorKind::Io,
            "no files selected".to_string(),
        )),
    }
}

pub fn copy_filename(app_state: &mut AppState) -> AppResult {
    let entry_file_name = app_state
        .state
        .tab_state_ref()
        .curr_tab_ref()
        .curr_list_ref()
        .and_then(|c| c.curr_entry_ref())
        .map(|entry| entry.file_name().to_string());

    if let Some(file_name) = entry_file_name {
        copy_string_to_buffer(file_name)?;
    }
    Ok(())
}

pub fn copy_filename_without_extension(app_state: &mut AppState) -> AppResult {
    let entry_file_name = app_state
        .state
        .tab_state_ref()
        .curr_tab_ref()
        .curr_list_ref()
        .and_then(|c| c.curr_entry_ref())
        .map(|entry| {
            entry
                .file_name()
                .rsplit_once('.')
                .map(|(name, _)| name.to_string())
                .unwrap_or_else(|| entry.file_name().to_string())
        });

    if let Some(file_name) = entry_file_name {
        copy_string_to_buffer(file_name)?;
    }
    Ok(())
}

pub fn copy_filepath(app_state: &mut AppState, all: bool) -> AppResult {
    let selected = app_state
        .state
        .tab_state_ref()
        .curr_tab_ref()
        .curr_list_ref();
    let entry_file_path = {
        if all {
            selected.map(|c| c.get_selected_paths()).and_then(|sel| {
                sel.into_iter().try_fold(String::new(), |mut acc, x| {
                    if let Some(s) = x.to_str() {
                        acc.push_str(s);
                        acc.push('\n');
                    }
                    Some(acc)
                })
            })
        } else {
            selected
                .and_then(|c| c.curr_entry_ref())
                .and_then(|entry| entry.file_path().to_str())
                .map(|s| s.to_string())
        }
    };
    if let Some(file_path) = entry_file_path {
        copy_string_to_buffer(file_path)?;
    }
    Ok(())
}

pub fn copy_dirpath(app_state: &mut AppState) -> AppResult {
    let opt_entry = app_state
        .state
        .tab_state_ref()
        .curr_tab_ref()
        .curr_list_ref()
        .map(|dirlist| dirlist.file_path());

    if let Some(s) = opt_entry.and_then(|p| p.to_str().map(String::from)) {
        copy_string_to_buffer(s)?
    };
    Ok(())
}

fn copy_string_to_buffer(string: String) -> AppResult {
    let clipboards = [
        (
            "wl-copy",
            format!("printf '%s' '{}' | {}", string, "wl-copy"),
        ),
        ("xsel", format!("printf '%s' '{}' | {} -ib", string, "xsel")),
        ("pbcopy", format!("printf '%s' '{}' | {}", string, "pbcopy")),
        (
            "xclip",
            format!(
                "printf '%s' '{}' | {} -selection clipboard",
                string, "xclip"
            ),
        ),
    ];

    for (_, cmd) in clipboards.iter() {
        let status = Command::new("sh")
            .args(["-c", cmd.as_str()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        match status {
            Ok(s) if s.success() => return Ok(()),
            _ => {}
        }
    }
    Err(AppError::new(
        AppErrorKind::Clipboard,
        "Failed to copy to clipboard".to_string(),
    ))
}
