use std::collections::HashMap;
use std::path;

use crate::error::AppResult;
use crate::types::state::{AppState, MatchState};

use super::cursor_move;

/// Flags shared by all `select_*` commands, controlling how the selection is applied.
#[derive(Clone, Copy, Debug)]
pub struct SelectOption {
    pub toggle: bool,
    pub all: bool,
    pub reverse: bool,
}

impl std::default::Default for SelectOption {
    fn default() -> Self {
        Self {
            toggle: true,
            all: false,
            reverse: false,
        }
    }
}

impl std::fmt::Display for SelectOption {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "--toggle={} --all={} --deselect={}",
            self.toggle, self.all, self.reverse
        )
    }
}

/// Fans selection decisions out to every open tab, keeping selection consistent across tabs.
///
/// Tabs hold independent copies of directory listings, so a selection change in one tab is
/// invisible in the others (e.g. deselecting in a new tab doesn't clear the original tab).
/// `decisions` maps entry paths to the permanent-selection state that was just applied in the
/// acting tab; the same *absolute* states are applied to matching entries in all tabs (the
/// acting tab's own entries are re-applied idempotently), so pre-diverged copies converge.
pub fn fan_out_selection(app_state: &mut AppState, decisions: &HashMap<path::PathBuf, bool>) {
    app_state
        .state
        .tab_state_mut()
        .iter_mut()
        .for_each(|(_, tab)| {
            tab.history_mut().values_mut().for_each(|list| {
                list.iter_mut()
                    .filter_map(|entry| {
                        decisions
                            .get(entry.file_path())
                            .map(|selected| (entry, *selected))
                    })
                    .for_each(|(entry, selected)| entry.set_permanent_selected(selected));
            });
        });
}

/// Applies `options` to entries matching `pattern`, or to the current entry/selection if
/// `pattern` is `None`. Shared by all `select_*` commands.
pub fn select_files(
    app_state: &mut AppState,
    pattern: &MatchState,
    options: &SelectOption,
) -> AppResult {
    if pattern.is_none() {
        select_without_pattern(app_state, options)
    } else {
        select_with_pattern(app_state, pattern, options)
    }
}

/// Applies `options` to every entry (`options.all`) or just the current entry, advancing the
/// cursor in the latter case.
fn select_without_pattern(app_state: &mut AppState, options: &SelectOption) -> AppResult {
    let mut decisions: HashMap<path::PathBuf, bool> = HashMap::new();
    if options.all {
        if let Some(curr_list) = app_state
            .state
            .tab_state_mut()
            .curr_tab_mut()
            .curr_list_mut()
        {
            curr_list.iter_mut().for_each(|e| {
                let selected = if options.reverse {
                    false
                } else if options.toggle {
                    !e.is_selected()
                } else {
                    true
                };
                e.set_permanent_selected(selected);
                decisions.insert(e.file_path_buf(), selected);
            });
        }
    } else if let Some(entry) = app_state
        .state
        .tab_state_mut()
        .curr_tab_mut()
        .curr_list_mut()
        .and_then(|s| s.curr_entry_mut())
    {
        let selected = if options.reverse {
            false
        } else if options.toggle {
            !entry.is_selected()
        } else {
            true
        };
        entry.set_permanent_selected(selected);
        decisions.insert(entry.file_path_buf(), selected);
        cursor_move::down(app_state, 1)?;
    }
    fan_out_selection(app_state, &decisions);
    Ok(())
}

/// Applies `options` to every entry matching `pattern`.
fn select_with_pattern(
    app_state: &mut AppState,
    pattern: &MatchState,
    options: &SelectOption,
) -> AppResult {
    let mut decisions: HashMap<path::PathBuf, bool> = HashMap::new();
    if let Some(curr_list) = app_state
        .state
        .tab_state_mut()
        .curr_tab_mut()
        .curr_list_mut()
    {
        let mut found = 0;
        curr_list
            .iter_mut()
            .filter(|e| pattern.is_match(e.file_name()))
            .for_each(|e| {
                found += 1;
                let selected = if options.reverse {
                    false
                } else if options.toggle {
                    !e.is_selected()
                } else {
                    true
                };
                e.set_permanent_selected(selected);
                decisions.insert(e.file_path_buf(), selected);
            });
        app_state
            .state
            .message_queue_mut()
            .push_info(format!("{} files selected", found));
    }
    fan_out_selection(app_state, &decisions);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path;

    use clap::Parser;
    use uuid::Uuid;

    use crate::commands::cursor_move;
    use crate::commands::uimodes;
    use crate::config::app::AppConfig;
    use crate::history::{generate_entries_to_root, DirectoryHistory, JoshutoHistory};
    use crate::tab::JoshutoTab;
    use crate::types::option::search::CaseSensitivity;
    use crate::types::state::{AppState, MatchState};
    use crate::Args;

    use super::{select_files, SelectOption};

    fn make_app_state() -> AppState {
        let args = Args::parse_from(["joshuto"]);
        AppState::new(AppConfig::default(), args)
    }

    /// Creates a tab on `cwd` and appends it last, leaving the active tab unchanged.
    fn spawn_tab(app_state: &mut AppState, cwd: &path::Path) {
        let id = Uuid::new_v4();
        let mut history = JoshutoHistory::new();
        let tab_options = app_state
            .config
            .display_options
            .default_tab_display_option
            .clone();
        let dirlists = generate_entries_to_root(
            cwd,
            &history,
            app_state.state.ui_state_ref(),
            &app_state.config.display_options,
            &tab_options,
        )
        .unwrap();
        history.insert_entries(dirlists);
        let tab = JoshutoTab::new(cwd.to_path_buf(), history, tab_options).unwrap();
        app_state.state.tab_state_mut().insert_tab(id, tab, true);
    }

    /// Switches the active tab by index directly (unlike `tab_switch`, this never touches the
    /// process cwd, which is global state other tests may depend on).
    fn switch_tab(app_state: &mut AppState, index: usize) {
        app_state.state.tab_state_mut().index = index;
    }

    /// Sorted names of the active tab's current-listing entries that are permanently selected.
    fn selected_names(app_state: &AppState) -> Vec<String> {
        let list = app_state
            .state
            .tab_state_ref()
            .curr_tab_ref()
            .curr_list_ref()
            .unwrap();
        let mut names: Vec<String> = list
            .iter()
            .filter(|e| e.is_permanent_selected())
            .map(|e| e.file_name().to_string())
            .collect();
        names.sort();
        names
    }

    fn temp_dir_with_files() -> path::PathBuf {
        let tmp = std::env::temp_dir().join(format!("joshuto_select_{}", Uuid::new_v4()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        for name in ["a_1", "a_2", "b_1"] {
            fs::write(tmp.join(name), b"x").unwrap();
        }
        tmp
    }

    #[test]
    fn select_all_fans_out_to_other_tabs() {
        let tmp = temp_dir_with_files();
        let mut app_state = make_app_state();
        spawn_tab(&mut app_state, &tmp); // tab A (active)
        spawn_tab(&mut app_state, &tmp); // tab B, same directory

        select_files(
            &mut app_state,
            &MatchState::default(),
            &SelectOption {
                all: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(selected_names(&app_state), vec!["a_1", "a_2", "b_1"]);
        switch_tab(&mut app_state, 1);
        assert_eq!(selected_names(&app_state), vec!["a_1", "a_2", "b_1"]);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn deselect_in_other_tab_clears_original_tab() {
        let tmp = temp_dir_with_files();
        let mut app_state = make_app_state();
        spawn_tab(&mut app_state, &tmp); // tab A (active)
        spawn_tab(&mut app_state, &tmp); // tab B, same directory

        // select everything in tab A
        select_files(
            &mut app_state,
            &MatchState::default(),
            &SelectOption {
                all: true,
                ..Default::default()
            },
        )
        .unwrap();

        // deselect everything in tab B (the reported bug: the original tab kept the selection)
        switch_tab(&mut app_state, 1);
        select_files(
            &mut app_state,
            &MatchState::default(),
            &SelectOption {
                all: true,
                reverse: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(selected_names(&app_state).is_empty());

        switch_tab(&mut app_state, 0);
        assert!(
            selected_names(&app_state).is_empty(),
            "deselection must propagate back to the original tab"
        );

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn current_entry_select_fans_out_to_other_tabs() {
        let tmp = temp_dir_with_files();
        let mut app_state = make_app_state();
        spawn_tab(&mut app_state, &tmp); // tab A (active)
        spawn_tab(&mut app_state, &tmp); // tab B, same directory

        // plain select on the cursor entry (default options: toggle, current entry only)
        select_files(
            &mut app_state,
            &MatchState::default(),
            &SelectOption::default(),
        )
        .unwrap();
        let selected = selected_names(&app_state);
        assert_eq!(selected.len(), 1, "exactly the cursor entry is selected");

        switch_tab(&mut app_state, 1);
        assert_eq!(selected_names(&app_state), selected);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn pattern_select_fans_out_to_other_tabs() {
        let tmp = temp_dir_with_files();
        let mut app_state = make_app_state();
        spawn_tab(&mut app_state, &tmp); // tab A (active)
        spawn_tab(&mut app_state, &tmp); // tab B, same directory

        let pattern = MatchState::new_glob("a_*", CaseSensitivity::Sensitive).unwrap();
        select_files(
            &mut app_state,
            &pattern,
            &SelectOption {
                toggle: false,
                ..Default::default()
            },
        )
        .unwrap();

        switch_tab(&mut app_state, 1);
        assert_eq!(selected_names(&app_state), vec!["a_1", "a_2"]);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn visual_mode_commit_fans_out_to_other_tabs() {
        let tmp = temp_dir_with_files();
        let mut app_state = make_app_state();
        spawn_tab(&mut app_state, &tmp); // tab A (active)
        spawn_tab(&mut app_state, &tmp); // tab B, same directory

        // enable visual mode, extend the range by one entry, then commit
        uimodes::toggle_visual_mode(&mut app_state).unwrap();
        cursor_move::down(&mut app_state, 1).unwrap();
        uimodes::toggle_visual_mode(&mut app_state).unwrap();

        let selected = selected_names(&app_state);
        assert_eq!(selected.len(), 2, "the visual range is committed");

        switch_tab(&mut app_state, 1);
        assert_eq!(selected_names(&app_state), selected);

        fs::remove_dir_all(&tmp).unwrap();
    }
}
