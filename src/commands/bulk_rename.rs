use std::env;
use std::fs;
use std::io::{BufRead, Write};
use std::path;
use std::process;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

use rand::Rng;

use crate::error::{AppError, AppErrorKind, AppResult};
use crate::types::state::remove_external_preview;
use crate::types::state::AppState;
use crate::ui::AppBackend;
use crate::utils::format::clear_screen;
use crate::utils::process::wait_for_enter;

use super::reload;

const ENV_TMP_DIR: &str = "TMP_DIR";
const ENV_EDITOR: &str = "EDITOR";
const ENV_VISUAL: &str = "VISUAL";
const FILE_PREFIX: &str = "joshuto-";
const RAND_STR_LEN: usize = 10;

/// Resolves the editor to open the bulk-rename buffer with: `$EDITOR`, then `$VISUAL`, then
/// the `nano` and `vi` fallbacks.
fn find_editor() -> AppResult<String> {
    let candidates = [
        env::var(ENV_EDITOR).ok(),
        env::var(ENV_VISUAL).ok(),
        Some("nano".to_string()),
        Some("vi".to_string()),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|e| !e.trim().is_empty())
        .ok_or_else(|| {
            AppError::new(
                AppErrorKind::EnvVar,
                "no editor found: set $EDITOR or $VISUAL".to_string(),
            )
        })
}

/// Writes the selected entries' names to a temp file, opens an editor on it, and applies any
/// line-for-line renames the user made, after a confirmation prompt.
pub fn _bulk_rename(app_state: &mut AppState) -> AppResult {
    let tmp_directory = env::var(ENV_TMP_DIR).unwrap_or_else(|_| "/tmp".to_string());

    let editor = find_editor()?;

    /* generate a random file name to write to */
    let mut rand_str = String::with_capacity(FILE_PREFIX.len() + RAND_STR_LEN);
    rand_str.push_str(FILE_PREFIX);
    rand::rng()
        .sample_iter(&rand::distr::Alphanumeric)
        .take(RAND_STR_LEN)
        .for_each(|ch| rand_str.push(ch as char));

    /* create this file in a temporary folder */
    let mut file_path = path::PathBuf::from(&tmp_directory);
    file_path.push(rand_str);

    let entries = app_state
        .state
        .tab_state_ref()
        .curr_tab_ref()
        .curr_list_ref()
        .map_or(vec![], |s| s.selected_or_current());

    /* write file names into temporary file to edit */
    {
        let mut file = fs::File::create(&file_path)?;
        for path in entries.iter() {
            if let Some(name) = path.file_path().file_name() {
                file.write_all(name.as_bytes())?;
                file.write_all(b"\n")?;
            }
        }
    }

    /* open file with text editor to edit */
    {
        let initial_modified = fs::metadata(&file_path)?.modified()?;

        process::Command::new(editor)
            .arg(&file_path)
            .spawn()?
            .wait()?;

        // check if the file was modified since it was created
        let last_modified = fs::metadata(&file_path)?.modified()?;
        if last_modified <= initial_modified {
            // remember to remove file
            let _ = std::fs::remove_file(&file_path);
            return Ok(());
        }
    }

    let mut paths_renamed: Vec<path::PathBuf> = Vec::with_capacity(entries.len());
    {
        let file = std::fs::File::open(&file_path)?;

        // Read raw bytes: file names may contain non-UTF-8 bytes (they were written verbatim
        // above), so `BufRead::lines()`'s UTF-8 validation would abort the whole rename.
        let reader = std::io::BufReader::new(file);
        for line in reader.split(b'\n') {
            let raw = line?;
            let bytes = trim_ascii(&raw);
            if bytes.is_empty() {
                continue;
            }
            #[cfg(unix)]
            let name = path::PathBuf::from(std::ffi::OsStr::from_bytes(bytes));
            #[cfg(not(unix))]
            let name = path::PathBuf::from(String::from_utf8_lossy(bytes).as_ref());
            paths_renamed.push(name);
        }
        // Remove the temp file as soon as we've read it: this is the only cleanup needed,
        // so a failed or declined rename below can't leave the file behind either.
        let _ = std::fs::remove_file(&file_path);
    }
    if paths_renamed.len() < entries.len() {
        return Err(AppError::new(
            AppErrorKind::Io,
            "Insufficient inputs".to_string(),
        ));
    }

    // Validate the new names before touching the filesystem: they must be plain file names
    // (no path separators, no `.`/`..`), unique among themselves, and must not overwrite
    // anything that already exists (on disk or under another selected entry's current name;
    // a single pass can't swap two names safely).
    let mut seen: std::collections::HashSet<std::ffi::OsString> =
        std::collections::HashSet::with_capacity(paths_renamed.len());
    for (p, q) in entries.iter().zip(paths_renamed.iter()) {
        let from = p.file_path().file_name();
        if from == Some(q.as_os_str()) {
            continue;
        }
        let is_plain_name = q.file_name() == Some(q.as_os_str())
            && !q.as_os_str().is_empty()
            && q.as_os_str() != std::ffi::OsStr::new(".")
            && q.as_os_str() != std::ffi::OsStr::new("..");
        if !is_plain_name {
            return Err(AppError::new(
                AppErrorKind::InvalidParameters,
                format!("invalid new file name: {:?}", q),
            ));
        }
        if !seen.insert(q.as_os_str().to_os_string()) {
            return Err(AppError::new(
                AppErrorKind::InvalidParameters,
                format!("duplicate new file name: {:?}", q),
            ));
        }
        if entries
            .iter()
            .any(|e| e.file_path().file_name() == Some(q.as_os_str()))
            || fs::symlink_metadata(q).is_ok()
        {
            return Err(AppError::new(
                AppErrorKind::InvalidParameters,
                format!("'{}' already exists", q.display()),
            ));
        }
    }

    clear_screen();
    for (p, q) in entries.iter().zip(paths_renamed.iter()) {
        println!("{:?} -> {:?}", p.file_name(), q);
    }
    print!("Continue with rename? (Y/n): ");
    std::io::stdout().flush()?;

    let mut user_input = String::with_capacity(4);
    std::io::stdin().read_line(&mut user_input)?;

    let user_input_fmt = user_input.trim().to_lowercase();
    match user_input_fmt.as_str() {
        "" | "y" | "yes" => {
            for (p, q) in entries.iter().zip(paths_renamed.iter()) {
                if let Some(from) = p.file_path().file_name() {
                    if from == q.as_os_str() {
                        continue;
                    }
                    fs::rename(from, q)?;
                }
            }
        }
        _ => {}
    }
    wait_for_enter()?;

    Ok(())
}

/// Trims leading and trailing ASCII whitespace from a byte slice.
fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while let Some(first) = bytes.first() {
        if first.is_ascii_whitespace() {
            bytes = &bytes[1..];
        } else {
            break;
        }
    }
    while let Some(last) = bytes.last() {
        if last.is_ascii_whitespace() {
            bytes = &bytes[..bytes.len() - 1];
        } else {
            break;
        }
    }
    bytes
}

/// Implements `bulk_rename`: releases the terminal for `$EDITOR`, runs the bulk rename, then
/// restores the terminal and reloads the current tab.
pub fn bulk_rename(app_state: &mut AppState, backend: &mut AppBackend) -> AppResult {
    remove_external_preview(app_state);
    backend.terminal_drop();
    let res = _bulk_rename(app_state);
    backend.terminal_restore()?;
    reload::soft_reload_curr_tab(app_state)?;
    res
}
