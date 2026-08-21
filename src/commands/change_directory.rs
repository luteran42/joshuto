use std::path::Path;

use crate::commands::{reload, zoxide};
use crate::error::AppResult;
use crate::preview::preview_dir::Background;
use crate::types::state::AppState;
use crate::utils::cwd;

/// Sets the process and tab working directory to `path`, updating zoxide's database if enabled.
/// Lower-level than [`change_directory`]: doesn't touch the directory-listing cache. Any pending
/// cursor target is cleared, since the tab has changed directories.
pub fn cd(path: &Path, app_state: &mut AppState, history_update: bool) -> std::io::Result<()> {
    cwd::set_current_dir(path)?;
    app_state
        .state
        .tab_state_mut()
        .curr_tab_mut()
        .set_cwd(path, history_update);
    // any pending cursor target is invalidated by a cwd change; callers that want to land on
    // an entry set it again explicitly after cd()
    let curr_tab = app_state.state.tab_state_mut().curr_tab_mut();
    curr_tab.pending_cursor = None;
    curr_tab.cancel_preview_load();
    if app_state.config.zoxide_update {
        debug_assert!(path.is_absolute());
        zoxide::zoxide_add(path.to_str().expect("cannot convert path to string"))?;
    }
    Ok(())
}

/// Implements `cd`: resolves `path` (relative, absolute, or a run of `../`) against the current
/// directory, changes into it, and kicks off background loading of listings for it and all its
/// ancestors.
pub fn change_directory(app_state: &mut AppState, path: &Path) -> AppResult {
    change_directory_impl(app_state, path, None)
}

/// Like [`change_directory`], but moves the cursor onto the entry named `cursor` once the new
/// listing has loaded.
pub fn change_directory_with_cursor(
    app_state: &mut AppState,
    path: &Path,
    cursor: String,
) -> AppResult {
    change_directory_impl(app_state, path, Some(cursor))
}

fn change_directory_impl(
    app_state: &mut AppState,
    mut path: &Path,
    cursor: Option<String>,
) -> AppResult {
    let new_cwd = if path.is_absolute() {
        path.to_path_buf()
    } else {
        while let Ok(p) = path.strip_prefix("../") {
            parent_directory(app_state)?;
            path = p;
        }

        let mut new_cwd = std::env::current_dir()?;
        new_cwd.push(path);
        new_cwd
    };

    cd(new_cwd.as_path(), app_state, true)?;
    let tab_id = app_state.state.tab_state_ref().curr_tab_id();
    if let Some(cursor) = cursor {
        if let Some(tab) = app_state.state.tab_state_mut().tab_mut(&tab_id) {
            tab.pending_cursor = Some(cursor);
        }
    }
    // load the new listings in the background so the UI stays responsive; the current pane
    // shows "loading..." until the result event arrives
    Background::load_directory(app_state, tab_id, new_cwd);
    Ok(())
}

/// Implements `cd ..`: changes into the parent of the current directory.
pub fn parent_directory(app_state: &mut AppState) -> AppResult {
    if let Some(parent) = app_state
        .state
        .tab_state_ref()
        .curr_tab_ref()
        .get_cwd()
        .parent()
        .map(|p| p.to_path_buf())
    {
        cd(&parent, app_state, true)?;
        reload::soft_reload_curr_tab(app_state)?;
    }
    Ok(())
}

/// Implements `cd -`: changes back to the directory this tab was in previously.
pub fn previous_directory(app_state: &mut AppState) -> AppResult {
    if let Some(path) = app_state
        .state
        .tab_state_ref()
        .curr_tab_ref()
        .previous_dir()
    {
        let path = path.to_path_buf();
        cd(&path, app_state, true)?;
        reload::soft_reload_curr_tab(app_state)?;
    }
    Ok(())
}
