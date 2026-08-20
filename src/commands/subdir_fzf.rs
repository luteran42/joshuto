use std::path::{Path, PathBuf};

use crate::error::AppResult;
use crate::types::state::AppState;
use crate::ui::AppBackend;

use super::change_directory::{change_directory, change_directory_with_cursor};
use super::fzf;

/// Implements `subdir_fzf`: opens an fzf picker over subdirectories and changes into the chosen
/// one.
pub fn subdir_fzf(app_state: &mut AppState, backend: &mut AppBackend) -> AppResult {
    let fzf_output = fzf::fzf(app_state, backend, Vec::new())?;
    let path: PathBuf = PathBuf::from(fzf_output);
    fzf_change_dir(app_state, path.as_path())
}

/// Changes into `path` if it's a directory, or into its parent with the cursor placed on it
/// otherwise.
pub fn fzf_change_dir(app_state: &mut AppState, path: &Path) -> AppResult {
    if path.is_dir() {
        change_directory(app_state, path)?;
    } else if let Some(parent) = path.parent() {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .trim();
        change_directory_with_cursor(app_state, parent, file_name.to_string())?;
    }
    Ok(())
}
