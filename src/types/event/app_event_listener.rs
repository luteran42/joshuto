use std::fmt::Debug;
use std::io;
use std::path;
use std::sync::mpsc;
use std::thread;

use ratatui::termion::event::Event;
use ratatui_image::protocol::Protocol;

use uuid::Uuid;

use crate::error::AppResult;
use crate::fs::{JoshutoDirEntry, JoshutoDirList};
use crate::preview::preview_file::FilePreview;
use crate::types::event::input_listener::TerminalInputListener;
use crate::types::event::signal_listener::SignalListener;
use crate::types::io::IoTaskProgressMessage;
use crate::types::io::IoTaskStat;

/// Sending half of the app event channel.
pub type AppEventSender = mpsc::Sender<AppEvent>;
/// Receiving half of the app event channel.
pub type AppEventReceiver = mpsc::Receiver<AppEvent>;

/// A successfully-generated file preview: either script/text output or a rendered image.
pub enum PreviewData {
    Script(Box<FilePreview>),
    Image(Box<Protocol>),
}

impl Debug for PreviewData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Script(_) => f.debug_tuple("Script").field(&"_").finish(),
            Self::Image(_) => f.debug_tuple("Image").field(&"_").finish(),
        }
    }
}

/// Every kind of asynchronous event the main loop can receive: terminal input, background IO
/// task updates, forked-process completion, preview results, terminal resize, and filesystem
/// changes.
#[derive(Debug)]
pub enum AppEvent {
    // User input events
    TerminalEvent(Event),

    // background IO worker events
    NewIoTask,
    IoTaskStart(IoTaskStat),
    IoTaskProgress(IoTaskProgressMessage),
    IoTaskResult(AppResult),

    // forked process events
    ChildProcessComplete(u32),

    // preview thread events
    PreviewDir {
        id: Uuid,
        path: path::PathBuf,
        res: Box<io::Result<JoshutoDirList>>,
    },
    PreviewFile {
        path: path::PathBuf,
        res: io::Result<PreviewData>,
    },
    // background directory-listing thread events
    LoadDirectory {
        id: Uuid,
        generation: u64,
        path: path::PathBuf,
        res: Box<io::Result<Vec<JoshutoDirEntry>>>,
    },
    // terminal size change events
    Signal(i32),
    // filesystem change events
    Filesystem(notify::Event),
}

//#[derive(Default, Debug, Clone, Copy)]
//pub struct Config {}

/// A small event handler that wrap termion input and tick events. Each event
/// type is handled in its own thread and returned to a common `Receiver`
pub struct AppEventListener {
    pub event_tx: AppEventSender,
    event_rx: AppEventReceiver,
    pub input_tx: mpsc::Sender<()>,
}

impl AppEventListener {
    /// Spawns the signal and terminal-input listener threads and returns the event channel.
    pub fn new() -> Self {
        Self::default()
    }

    // We need a next() and a flush() so we don't continuously consume
    // input from the console. Sometimes, other applications need to
    // read terminal inputs while joshuto is in the background
    /// Blocks until the next `AppEvent` arrives.
    pub fn next(&self) -> Result<AppEvent, mpsc::RecvError> {
        let event = self.event_rx.recv()?;
        Ok(event)
    }

    /// Signals the input thread to poll for the next terminal input event.
    pub fn flush(&self) {
        loop {
            if self.input_tx.send(()).is_ok() {
                break;
            }
        }
    }
}

impl std::default::Default for AppEventListener {
    fn default() -> Self {
        let (input_tx, input_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        // signal thread
        let signal_listener = SignalListener::new(event_tx.clone());
        let _ = thread::spawn(move || {
            signal_listener.run();
        });

        // edge case that starts off the input thread
        let _ = input_tx.send(());
        // input thread
        let input_listener = TerminalInputListener::new(event_tx.clone(), input_rx);
        let _ = thread::spawn(move || {
            input_listener.run();
        });

        AppEventListener {
            event_tx,
            event_rx,
            input_tx,
        }
    }
}

#[cfg(test)]
mod preview_after_cd_repro {
    use std::fs;
    use std::time::{Duration, Instant};

    use clap::Parser;
    use uuid::Uuid;

    use crate::commands::change_directory::change_directory;
    use crate::config::app::AppConfig;
    use crate::history::{generate_entries_to_root, DirectoryHistory, JoshutoHistory};
    use crate::run::process_event;
    use crate::tab::JoshutoTab;
    use crate::types::event::AppEvent;
    use crate::types::state::AppState;
    use crate::Args;

    fn make_app_state() -> AppState {
        let args = Args::parse_from(["joshuto"]);
        AppState::new(AppConfig::default(), args)
    }

    fn spawn_tab(app_state: &mut AppState, cwd: &std::path::Path) {
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

    /// Mimics run_loop's event dispatch: processes non-terminal events and, after a
    /// LoadDirectory, kicks off the cursor-entry preview exactly like the run loop does.
    fn pump_until(
        app_state: &mut AppState,
        deadline: Duration,
        mut done: impl FnMut(&AppState) -> bool,
    ) {
        let start = Instant::now();
        while start.elapsed() < deadline {
            let event = match app_state
                .events
                .event_rx
                .recv_timeout(Duration::from_millis(50))
            {
                Ok(event) => event,
                Err(_) => continue,
            };
            let is_directory_load = matches!(event, AppEvent::LoadDirectory { .. });
            match event {
                AppEvent::TerminalEvent(_) => {}
                event => process_event::process_noninteractive(event, app_state),
            }
            if is_directory_load {
                // run_loop: preview_default::load_previews(app_state, backend), dir case only
                let curr_tab = app_state.state.tab_state_ref().curr_tab_ref();
                if let Some(list) = curr_tab.curr_list_ref() {
                    if let Some(index) = list.get_index() {
                        let entry = &list.contents[index];
                        if entry.metadata.is_dir() {
                            let p = entry.file_path().to_path_buf();
                            let need_to_load = curr_tab
                                .history_metadata_ref()
                                .get(p.as_path())
                                .map(|m| !m.is_loading())
                                .unwrap_or(true)
                                && curr_tab
                                    .history_ref()
                                    .get(p.as_path())
                                    .map(|e| e.need_update())
                                    .unwrap_or(true);
                            if need_to_load {
                                crate::preview::preview_dir::Background::load_preview(app_state, p);
                            }
                        }
                    }
                }
            }
            if done(app_state) {
                return;
            }
        }
    }

    #[test]
    fn jump_to_big_dir_shows_cursor_entry_preview() {
        let tmp =
            std::env::temp_dir().join(format!("joshuto_preview_repro_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let zenek = tmp.join("zenek");
        fs::create_dir_all(&zenek).unwrap();
        for i in 0..300 {
            fs::write(zenek.join(format!("track_{:05}.flac", i)), b"x").unwrap();
            let album = zenek.join(format!("album_{:03}", i));
            fs::create_dir_all(&album).unwrap();
            fs::write(album.join("01.wav"), b"x").unwrap();
        }

        let mut app_state = make_app_state();
        spawn_tab(&mut app_state, &tmp);

        change_directory(&mut app_state, &zenek).unwrap();

        // 1. the cwd listing must land on its own
        pump_until(&mut app_state, Duration::from_secs(10), |state| {
            state
                .state
                .tab_state_ref()
                .curr_tab_ref()
                .curr_list_ref()
                .is_some()
        });
        let tab = app_state.state.tab_state_ref().curr_tab_ref();
        let list = tab.curr_list_ref().expect("cwd listing present");
        assert_eq!(list.contents.len(), 600, "all entries listed");
        assert_eq!(list.get_index(), Some(0));
        assert!(
            tab.history_metadata_ref().get(zenek.as_path()).is_none(),
            "cwd Loading state cleared"
        );

        // 2. the cursor entry's directory preview must be spawned and land without further input
        pump_until(&mut app_state, Duration::from_secs(10), |state| {
            state
                .state
                .tab_state_ref()
                .curr_tab_ref()
                .child_list_ref()
                .is_some()
        });
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fast_cursor_movement_does_not_poison_preview_loading() {
        use crate::commands::cursor_move::cursor_move;

        let tmp =
            std::env::temp_dir().join(format!("joshuto_fast_scroll_repro_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        for i in 0..5 {
            let sub = tmp.join(format!("folder_{:02}", i));
            fs::create_dir_all(&sub).unwrap();
            fs::write(sub.join("file.txt"), b"hello").unwrap();
        }

        let mut app_state = make_app_state();
        spawn_tab(&mut app_state, &tmp);

        // Rapidly scroll across folders 0 -> 1 -> 2 -> 3
        for i in 0..=3 {
            cursor_move(&mut app_state, i);
            let tab = app_state.state.tab_state_ref().curr_tab_ref();
            if let Some(entry) = tab.curr_list_ref().and_then(|l| l.curr_entry_ref()) {
                let p = entry.file_path().to_path_buf();
                crate::preview::preview_dir::Background::load_preview(&mut app_state, p);
            }
        }

        // The final folder (folder_03) preview must complete
        pump_until(&mut app_state, Duration::from_secs(5), |state| {
            state
                .state
                .tab_state_ref()
                .curr_tab_ref()
                .child_list_ref()
                .is_some()
        });

        let tab = app_state.state.tab_state_ref().curr_tab_ref();
        let child = tab.child_list_ref().expect("folder_03 preview loaded");
        assert_eq!(child.contents.len(), 1);

        // Skipped folders must not be stuck in Loading state
        for i in 0..3 {
            let skipped_path = tmp.join(format!("folder_{:02}", i));
            assert!(
                tab.history_metadata_ref().get(&skipped_path).is_none(),
                "skipped folder_{:02} loading state was properly cleaned up",
                i
            );
        }

        // Moving back to a previously skipped folder (folder_01) must load its preview cleanly
        cursor_move(&mut app_state, 1);
        let tab = app_state.state.tab_state_ref().curr_tab_ref();
        let entry = tab
            .curr_list_ref()
            .and_then(|l| l.curr_entry_ref())
            .unwrap();
        let p = entry.file_path().to_path_buf();
        crate::preview::preview_dir::Background::load_preview(&mut app_state, p);

        pump_until(&mut app_state, Duration::from_secs(5), |state| {
            state
                .state
                .tab_state_ref()
                .curr_tab_ref()
                .child_list_ref()
                .is_some()
        });

        let tab = app_state.state.tab_state_ref().curr_tab_ref();
        let child = tab.child_list_ref().expect("folder_01 preview loaded");
        assert_eq!(child.contents.len(), 1);

        let _ = fs::remove_dir_all(&tmp);
    }
}
