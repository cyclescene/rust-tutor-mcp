use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};

use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebouncedEventKind};
use similar::{ChangeTag, TextDiff};

use crate::store::{FileChangeRecord, TutorStore};

pub struct FileWatcher {}

impl FileWatcher {
    pub fn spawn(store: Arc<Mutex<TutorStore>>) -> JoinHandle<()> {
        std::thread::spawn(move || {
            let Some(root) = detect_project_root() else {
                tracing::warn!("could not detect project root — file watcher will not run");
                return;
            };

            let mut state = WatcherState::new(&root, store);

            let (tx, rx) = std::sync::mpsc::channel();
            let mut debounder =
                new_debouncer(Duration::from_millis(500), tx).expect("failed to create debouncer");

            debounder
                .watcher()
                .watch(&root, RecursiveMode::Recursive)
                .expect("failed to watch project root");

            for result in rx {
                match result {
                    Ok(events) => {
                        for event in events {
                            if event.path.extension().and_then(|e| e.to_str()) == Some("rs") {
                                match event.kind {
                                    DebouncedEventKind::Any | DebouncedEventKind::AnyContinuous => {
                                        if event.path.exists() {
                                            state.process_event(&event.path);
                                        } else {
                                            state.last_seen.remove(&event.path);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("file watcher error: {e}");
                    }
                }
            }
            let _ = debounder;
        })
    }
}

struct WatcherState {
    last_seen: HashMap<PathBuf, String>,
    db: Arc<Mutex<TutorStore>>,
}

impl WatcherState {
    fn new(root: &Path, db: Arc<Mutex<TutorStore>>) -> Self {
        let mut last_seen = HashMap::new();

        // walk the project and seed the last seen map
        for entry in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("rs"))
        {
            if let Ok(contents) = std::fs::read_to_string(entry.path()) {
                last_seen.insert(entry.path().to_path_buf(), contents);
            }
        }

        Self { last_seen, db }
    }

    fn process_event(&mut self, path: &Path) {
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(e) => {
                tracing::error!("failed to read file: {e}");
                return;
            }
        };

        let empty = String::new();
        let old = self.last_seen.get(path).unwrap_or(&empty);

        if old == &contents {
            return;
        }

        let hunks = extract_hunks(old, &contents);

        let change_id = uuid::Uuid::new_v4().to_string();
        for hunk in hunks {
            let record = FileChangeRecord {
                id: 0,
                file_path: path.to_str().unwrap().to_string(),
                hunk_idx: hunk.idx as i64,
                change_id: change_id.clone(),
                old_start: hunk.old_start,
                old_count: hunk.old_count,
                new_start: hunk.new_start,
                new_count: hunk.new_count,
                before_lines: hunk.before_lines,
                after_lines: hunk.after_lines,
                changed_at: chrono::Utc::now(),
            };

            self.db
                .lock()
                .expect("store lock poisoned")
                .save_file_change(&record)
                .expect("failed to save file change");
        }

        self.last_seen.insert(path.to_path_buf(), contents);
    }
}

struct HunkData {
    idx: usize,
    old_start: i64,
    old_count: i64,
    new_start: i64,
    new_count: i64,
    before_lines: String,
    after_lines: String,
}

fn extract_hunks(old: &str, new: &str) -> Vec<HunkData> {
    let diff = TextDiff::from_lines(old, new);
    let mut hunks = Vec::new();
    for (idx, hunk) in diff.unified_diff().iter_hunks().enumerate() {
        let ops = hunk.ops();

        let old_start = ops.first().map(|op| op.old_range().start).unwrap_or(0) as i64;
        let old_end = ops.last().map(|op| op.old_range().end).unwrap_or(0) as i64;
        let new_start = ops.first().map(|op| op.new_range().start).unwrap_or(0) as i64;
        let new_end = ops.last().map(|op| op.new_range().end).unwrap_or(0) as i64;

        let mut before_lines = Vec::new();
        let mut after_lines = Vec::new();
        for change in hunk.iter_changes() {
            match change.tag() {
                ChangeTag::Delete => before_lines.push(change.value().to_string()),
                ChangeTag::Insert => after_lines.push(change.value().to_string()),
                ChangeTag::Equal => {}
            }
        }

        hunks.push(HunkData {
            idx,
            old_start,
            old_count: old_end - old_start,
            new_start,
            new_count: new_end - new_start,
            before_lines: before_lines.join("\n"),
            after_lines: after_lines.join("\n"),
        });
    }

    hunks
}

fn detect_project_root() -> Option<PathBuf> {
    Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| PathBuf::from(s.trim()))
}
