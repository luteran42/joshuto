use std::path::{self, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use lazy_static::lazy_static;
use uuid::Uuid;

use crate::fs::JoshutoDirList;
use crate::history::read_directory;
use crate::tab::TabDisplayOption;
use crate::types::event::AppEvent;
use crate::types::option::display::DisplayOption;
use crate::types::state::AppState;

/// Status of a directory preview being generated on a background thread.
#[derive(Debug, Clone)]
pub enum PreviewDirState {
    Loading,
    Error { message: String },
}

impl PreviewDirState {
    /// Returns `true` if the preview is still being generated.
    pub fn is_loading(&self) -> bool {
        matches!(*self, Self::Loading)
    }
}

enum DirTask {
    LoadDirectory {
        tab_id: Uuid,
        generation: u64,
        path: PathBuf,
        options: DisplayOption,
        tab_options: TabDisplayOption,
        event_tx: Sender<AppEvent>,
        cancel_token: Arc<AtomicBool>,
    },
    LoadPreview {
        tab_id: Uuid,
        path: PathBuf,
        options: DisplayOption,
        tab_options: TabDisplayOption,
        event_tx: Sender<AppEvent>,
        cancel_token: Arc<AtomicBool>,
    },
}

impl DirTask {
    fn run(self) {
        match self {
            DirTask::LoadDirectory {
                tab_id,
                generation,
                path,
                options,
                tab_options,
                event_tx,
                cancel_token,
            } => {
                if cancel_token.load(Ordering::Relaxed) {
                    return;
                }
                let filter_func = options.filter_func();
                let dir_res = read_directory(&path, filter_func, &options, &tab_options);
                if cancel_token.load(Ordering::Relaxed) {
                    return;
                }
                let res = AppEvent::LoadDirectory {
                    id: tab_id,
                    generation,
                    path,
                    res: Box::new(dir_res),
                };
                let _ = event_tx.send(res);
            }
            DirTask::LoadPreview {
                tab_id,
                path,
                options,
                tab_options,
                event_tx,
                cancel_token,
            } => {
                if cancel_token.load(Ordering::Relaxed) {
                    return;
                }
                let dir_res = JoshutoDirList::from_path(path.clone(), &options, &tab_options);
                if cancel_token.load(Ordering::Relaxed) {
                    return;
                }
                let res = AppEvent::PreviewDir {
                    id: tab_id,
                    path,
                    res: Box::new(dir_res),
                };
                let _ = event_tx.send(res);
            }
        }
    }
}

lazy_static! {
    static ref DIR_WORKER_TX: Sender<DirTask> = {
        let (tx, rx) = mpsc::channel::<DirTask>();
        let rx = Arc::new(Mutex::new(rx));
        let num_workers = num_cpus::get().clamp(2, 4);
        for _ in 0..num_workers {
            let rx_clone = Arc::clone(&rx);
            thread::spawn(move || {
                while let Ok(task) = {
                    let lock = rx_clone.lock().unwrap();
                    lock.recv()
                } {
                    task.run();
                }
            });
        }
        tx
    };
}

/// Namespace for queuing background directory-preview and directory-listing loads.
pub struct Background {}

impl Background {
    /// Queues a directory preview load on the worker pool for `dir_path` and posts an
    /// [`AppEvent::PreviewDir`] when done, cancelling any prior in-flight preview for this tab.
    pub fn load_preview(app_state: &mut AppState, dir_path: path::PathBuf) {
        let event_tx = app_state.events.event_tx.clone();
        let options = app_state.config.display_options.clone();
        let tab_id = app_state.state.tab_state_ref().curr_tab_id();
        let tab_options = app_state
            .state
            .tab_state_ref()
            .tab_ref(&tab_id)
            .map(|t| t.option_ref().clone())
            .unwrap_or_default();

        let cancel_token = Arc::new(AtomicBool::new(false));
        if let Some(tab) = app_state.state.tab_state_mut().tab_mut(&tab_id) {
            // Cancel previous in-flight preview request for this tab and clear its loading state
            tab.cancel_preview_load();
            tab.preview_in_flight = Some((dir_path.clone(), cancel_token.clone()));
            tab.history_metadata_mut()
                .insert(dir_path.clone(), PreviewDirState::Loading);
        }

        let _ = DIR_WORKER_TX.send(DirTask::LoadPreview {
            tab_id,
            path: dir_path,
            options,
            tab_options,
            event_tx,
            cancel_token,
        });
    }

    /// Queues a raw directory listing load on the worker pool for `dir_path` and posts an
    /// [`AppEvent::LoadDirectory`] when done, cancelling any prior in-flight load for this tab.
    pub fn load_directory(app_state: &mut AppState, tab_id: Uuid, dir_path: path::PathBuf) {
        let event_tx = app_state.events.event_tx.clone();
        let options = app_state.config.display_options.clone();
        let tab_options = app_state
            .state
            .tab_state_ref()
            .tab_ref(&tab_id)
            .map(|t| t.option_ref().clone())
            .unwrap_or_default();

        let cached = app_state
            .state
            .tab_state_ref()
            .tab_ref(&tab_id)
            .map(|t| t.history_ref().contains_key(dir_path.as_path()))
            .unwrap_or(false);

        let cancel_token = Arc::new(AtomicBool::new(false));
        let generation = if let Some(tab) = app_state.state.tab_state_mut().tab_mut(&tab_id) {
            // Cancel previous in-flight directory load for this tab and clear its loading state
            tab.cancel_dir_load();
            tab.dir_load_in_flight = Some((dir_path.clone(), cancel_token.clone()));
            if !cached {
                tab.history_metadata_mut()
                    .insert(dir_path.clone(), PreviewDirState::Loading);
            }
            tab.load_generation += 1;
            tab.load_generation
        } else {
            0
        };

        let _ = DIR_WORKER_TX.send(DirTask::LoadDirectory {
            tab_id,
            generation,
            path: dir_path,
            options,
            tab_options,
            event_tx,
            cancel_token,
        });
    }
}
