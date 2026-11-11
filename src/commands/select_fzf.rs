use std::collections::HashMap;

use crate::error::{AppError, AppErrorKind, AppResult};
use crate::types::state::AppState;
use crate::ui::AppBackend;

use super::fzf;
use super::select::fan_out_selection;
use super::select::SelectOption;

/// Implements `select_fzf`: opens a multi-select fzf picker over the current directory's
/// entries and applies the selection (or deselection/toggle, per `options`).
pub fn select_fzf(
    app_state: &mut AppState,
    backend: &mut AppBackend,
    options: &SelectOption,
) -> AppResult {
    let items = app_state
        .state
        .tab_state_ref()
        .curr_tab_ref()
        .curr_list_ref()
        .map(|list| {
            let v: Vec<String> = list
                .iter()
                .enumerate()
                .map(|(i, entry)| format!("{} {}\n", i, entry.file_name()))
                .collect();
            v
        })
        .unwrap_or_default();

    if items.is_empty() {
        return Err(AppError::new(
            AppErrorKind::Io,
            "no files to select".to_string(),
        ));
    }

    let fzf_output = fzf::fzf_multi(app_state, backend, items)?;

    let mut decisions: HashMap<std::path::PathBuf, bool> = HashMap::new();
    {
        let mut found = 0;

        if let Some(curr_list) = app_state
            .state
            .tab_state_mut()
            .curr_tab_mut()
            .curr_list_mut()
        {
            for selected in fzf_output.lines() {
                if let Some((selected_idx_str, _)) = selected.split_once(' ') {
                    if let Some(entry) = selected_idx_str
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| curr_list.contents.get_mut(index))
                    {
                        found += 1;

                        let new_selected = if options.reverse {
                            false
                        } else if options.toggle {
                            !entry.is_selected()
                        } else {
                            true
                        };
                        entry.set_permanent_selected(new_selected);
                        decisions.insert(entry.file_path_buf(), new_selected);
                    }
                }
            }
        }

        app_state
            .state
            .message_queue_mut()
            .push_info(format!("{} files selected", found));
    }

    // Keep the selection consistent in every other tab listing these paths.
    fan_out_selection(app_state, &decisions);

    Ok(())
}
