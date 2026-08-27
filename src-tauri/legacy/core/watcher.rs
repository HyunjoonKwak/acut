use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Represents a single file-system change event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchEvent {
    pub path: String,
    pub kind: String, // "create", "modify", "remove"
    pub timestamp: String,
}

/// Manages file-system watchers for one or more directories.
///
/// Each watched directory has its own `RecommendedWatcher` plus a background
/// debounce thread that batches raw events into `WatchEvent` groups and
/// delivers them through a user-supplied callback.
pub struct WatcherManager {
    watchers: Mutex<HashMap<String, WatcherHandle>>,
    events: Mutex<Vec<WatchEvent>>,
}

/// Internal handle that keeps the watcher and the shutdown signal alive.
struct WatcherHandle {
    _watcher: RecommendedWatcher,
    /// Dropping the sender signals the debounce thread to stop.
    _shutdown_tx: mpsc::Sender<()>,
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Map a `notify::EventKind` to one of the three canonical kind strings.
fn event_kind_label(kind: &EventKind) -> Option<&'static str> {
    match kind {
        EventKind::Create(_) => Some("create"),
        EventKind::Modify(_) => Some("modify"),
        EventKind::Remove(_) => Some("remove"),
        _ => None,
    }
}

/// Convert a `notify::Event` into zero or more `WatchEvent`s (one per path).
fn into_watch_events(event: &Event) -> Vec<WatchEvent> {
    let Some(kind_label) = event_kind_label(&event.kind) else {
        return Vec::new();
    };

    let timestamp = chrono::Local::now().to_rfc3339();

    event
        .paths
        .iter()
        .map(|p| WatchEvent {
            path: p.to_string_lossy().into_owned(),
            kind: kind_label.to_string(),
            timestamp: timestamp.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// WatcherManager implementation
// ---------------------------------------------------------------------------

impl WatcherManager {
    /// Create a new, empty manager.
    pub fn new() -> Self {
        Self {
            watchers: Mutex::new(HashMap::new()),
            events: Mutex::new(Vec::new()),
        }
    }

    /// Begin watching `folder_path` recursively.
    ///
    /// File-system events are debounced (500 ms window) and delivered as a
    /// `Vec<WatchEvent>` batch to `callback`.  The callback is invoked on a
    /// dedicated background thread.
    ///
    /// Calling this again with the same path replaces the previous watcher.
    pub fn start_watching(
        &self,
        folder_path: &str,
        callback: impl Fn(Vec<WatchEvent>) + Send + 'static,
    ) -> Result<(), String> {
        let canonical = PathBuf::from(folder_path)
            .canonicalize()
            .map_err(|e| format!("Invalid path '{}': {}", folder_path, e))?;
        let key = canonical.to_string_lossy().to_string();

        // If already watching this path, stop the old watcher first.
        {
            let mut map = self.watchers.lock().unwrap();
            map.remove(&key); // drop the old handle which stops the watcher + thread
        }

        // Channel: watcher -> debounce thread
        let (event_tx, event_rx) = mpsc::channel::<Event>();

        // Channel: used only for shutdown signalling (sender kept in the handle)
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

        // --- debounce thread ---
        let debounce_duration = Duration::from_millis(500);

        std::thread::Builder::new()
            .name(format!("watcher-debounce-{}", key))
            .spawn(move || {
                let mut pending: Vec<WatchEvent> = Vec::new();
                let mut last_event_time = Instant::now();

                loop {
                    // Check if we should shut down (sender dropped).
                    if shutdown_rx.try_recv().is_ok() {
                        break;
                    }

                    // Try to receive events with a short timeout so we can
                    // periodically flush the pending batch.
                    match event_rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(event) => {
                            let mut watch_events = into_watch_events(&event);
                            if !watch_events.is_empty() {
                                pending.append(&mut watch_events);
                                last_event_time = Instant::now();
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            // Nothing received -- check if it's time to flush.
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            // Watcher was dropped; flush remaining and exit.
                            if !pending.is_empty() {
                                callback(pending);
                            }
                            break;
                        }
                    }

                    // Flush if we have pending events and the debounce window elapsed.
                    if !pending.is_empty() && last_event_time.elapsed() >= debounce_duration {
                        let batch = std::mem::take(&mut pending);
                        callback(batch);
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn debounce thread: {}", e))?;

        // --- create the watcher ---
        let config = Config::default().with_poll_interval(Duration::from_millis(500));

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    // Best-effort send; if the debounce thread is gone the
                    // event is simply dropped.
                    let _ = event_tx.send(event);
                }
            },
            config,
        )
        .map_err(|e| format!("Failed to create watcher: {}", e))?;

        watcher
            .watch(canonical.as_path(), RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch '{}': {}", key, e))?;

        log::info!("Started watching: {}", key);

        // Store the handle (dropping it later will stop the watcher).
        {
            let mut map = self.watchers.lock().unwrap();
            map.insert(
                key,
                WatcherHandle {
                    _watcher: watcher,
                    _shutdown_tx: shutdown_tx,
                },
            );
        }

        Ok(())
    }

    /// Stop watching `folder_path`.  Returns an error if the path was not
    /// being watched.
    pub fn stop_watching(&self, folder_path: &str) -> Result<(), String> {
        let canonical = PathBuf::from(folder_path)
            .canonicalize()
            .map_err(|e| format!("Invalid path '{}': {}", folder_path, e))?;
        let key = canonical.to_string_lossy().to_string();

        let mut map = self.watchers.lock().unwrap();
        if map.remove(&key).is_some() {
            log::info!("Stopped watching: {}", key);
            Ok(())
        } else {
            Err(format!("Not watching '{}'", key))
        }
    }

    /// List the canonical paths of all directories currently being watched.
    pub fn get_watched_folders(&self) -> Vec<String> {
        let map = self.watchers.lock().unwrap();
        map.keys().cloned().collect()
    }

    /// Record events into the shared log (call from the debounce callback if
    /// you want events stored for later retrieval via `get_recent_events`).
    pub fn record_events(&self, events: &[WatchEvent]) {
        let mut store = self.events.lock().unwrap();
        store.extend(events.iter().cloned());

        // Cap the log to a reasonable size so memory doesn't grow unbounded.
        const MAX_STORED_EVENTS: usize = 10_000;
        if store.len() > MAX_STORED_EVENTS {
            let excess = store.len() - MAX_STORED_EVENTS;
            store.drain(..excess);
        }
    }

    /// Return the last `limit` recorded events (newest last).
    pub fn get_recent_events(&self, limit: usize) -> Vec<WatchEvent> {
        let store = self.events.lock().unwrap();
        let start = store.len().saturating_sub(limit);
        store[start..].to_vec()
    }
}
