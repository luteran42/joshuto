use std::collections::HashMap;
use std::path;

use crate::error::AppResult;
use crate::types::state::AppState;

use super::select::fan_out_selection;

/// Implements `toggle_visual_mode`: enters or exits visual (range) selection mode.
///
/// When visual mode is committed (exited with the range kept), the promoted selection is
/// fanned out to every open tab, like all selection changes.
pub fn toggle_visual_mode(app_state: &mut AppState) -> AppResult {
    let mut committed: Vec<path::PathBuf> = Vec::new();
    if let Some(curr_dir_list) = app_state
        .state
        .tab_state_mut()
        .curr_tab_mut()
        .curr_list_mut()
    {
        committed = curr_dir_list.toggle_visual_mode();
    };
    if !committed.is_empty() {
        let decisions: HashMap<path::PathBuf, bool> =
            committed.into_iter().map(|p| (p, true)).collect();
        fan_out_selection(app_state, &decisions);
    }
    Ok(())
}
