mod commands;
pub mod crypto;
pub mod error;
mod export;
mod import;
pub mod lock;
pub mod media;
mod protocol;
pub mod vault;
mod winlock;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use zeroize::Zeroizing;

use crate::error::{Error, Result};
use crate::lock::{LockInfo, WriterLock};
use crate::media::Tools;
use crate::vault::{now_secs, Index, Vault};

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub vault_path: Option<String>,
    /// Minuten ohne Bedienung bis zur automatischen Sperre (0 = nie).
    pub auto_lock_minutes: u32,
    /// Fenster vor Bildschirmaufnahmen/Screensharing schützen.
    pub content_protection: bool,
    /// Tresor sperren, sobald Windows gesperrt wird (Win+L) oder in den Standby geht.
    pub lock_on_windows_lock: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { vault_path: None, auto_lock_minutes: 10, content_protection: true, lock_on_windows_lock: true }
    }
}

/// Ein entsperrter Tresor. Wird beim Sperren verworfen; die Schlüssel werden
/// dabei durch `Zeroizing` überschrieben.
pub struct Session {
    pub vault: RwLock<Vault>,
    pub index: RwLock<Index>,
    pub writer: Mutex<Option<WriterLock>>,
    pub holder: Mutex<Option<LockInfo>>,
    index_stamp: Mutex<Option<SystemTime>>,
    pub busy: AtomicBool,
    last_heartbeat: AtomicU64,
    png_cache: Mutex<Vec<(String, Arc<Zeroizing<Vec<u8>>>)>>,
}

impl Session {
    fn new(vault: Vault, index: Index, writer: Option<WriterLock>, holder: Option<LockInfo>) -> Self {
        let stamp = index_mtime(&vault);
        Self {
            vault: RwLock::new(vault),
            index: RwLock::new(index),
            writer: Mutex::new(writer),
            holder: Mutex::new(holder),
            index_stamp: Mutex::new(stamp),
            busy: AtomicBool::new(false),
            last_heartbeat: AtomicU64::new(now_secs()),
            png_cache: Mutex::new(Vec::new()),
        }
    }

    pub fn read_only(&self) -> bool {
        self.writer.lock().unwrap().is_none()
    }

    /// Ändert den Index und speichert ihn sofort. Nur mit Schreibsperre möglich.
    pub fn mutate<T>(&self, f: impl FnOnce(&mut Index) -> Result<T>) -> Result<T> {
        let vault = self.vault.read().unwrap();
        let mut writer = self.writer.lock().unwrap();
        let Some(w) = writer.as_ref() else { return Err(Error::ReadOnly) };
        if !w.still_ours(&vault) {
            *writer = None;
            *self.holder.lock().unwrap() = lock::read_lock(&vault).ok().flatten();
            return Err(Error::ReadOnly);
        }
        let mut idx = self.index.write().unwrap();
        let mut copy = idx.clone();
        let out = f(&mut copy)?;
        copy.version += 1;
        vault.save_index(&copy)?;
        *idx = copy;
        *self.index_stamp.lock().unwrap() = index_mtime(&vault);
        Ok(out)
    }

    /// TIFF/BMP -> PNG, die letzten Umwandlungen bleiben im RAM.
    pub fn display_png(&self, id: &str) -> Result<Arc<Zeroizing<Vec<u8>>>> {
        if let Some((_, data)) = self.png_cache.lock().unwrap().iter().find(|(i, _)| i == id) {
            return Ok(data.clone());
        }
        let original = self.vault.read().unwrap().read_all(id, vault::ObjKind::Original)?;
        let png = Arc::new(Zeroizing::new(media::to_display_png(&original)?));
        let mut cache = self.png_cache.lock().unwrap();
        cache.insert(0, (id.to_string(), png.clone()));
        cache.truncate(4);
        Ok(png)
    }

    fn release(&self) {
        let vault = self.vault.read().unwrap();
        if let Some(w) = self.writer.lock().unwrap().take() {
            w.release(&vault);
        }
    }
}

fn index_mtime(vault: &Vault) -> Option<SystemTime> {
    std::fs::metadata(vault.index_path()).and_then(|m| m.modified()).ok()
}

pub struct AppState {
    session: Mutex<Option<Arc<Session>>>,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    last_activity: AtomicU64,
    pub tools: Tools,
    pub cancel: Arc<AtomicBool>,
}

impl AppState {
    pub fn current(&self) -> Option<Arc<Session>> {
        self.session.lock().unwrap().clone()
    }

    pub fn session(&self) -> Result<Arc<Session>> {
        self.current().ok_or(Error::Locked)
    }

    pub fn touch(&self) {
        self.last_activity.store(now_secs(), Ordering::Relaxed);
    }

    pub fn set_session(&self, s: Session) {
        self.touch();
        if let Some(old) = self.session.lock().unwrap().replace(Arc::new(s)) {
            old.release();
        }
    }

    pub fn lock(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(old) = self.session.lock().unwrap().take() {
            old.release();
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub fn save_settings(&self, s: Settings) -> Result<()> {
        if let Some(dir) = self.settings_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        vault::write_atomic(&self.settings_path, &serde_json::to_vec_pretty(&s)?)?;
        *self.settings.lock().unwrap() = s;
        Ok(())
    }
}

/// Hintergrund-Wächter: Auto-Sperre, Heartbeat der Schreibsperre und
/// Nachladen des Index, wenn ein anderer PC etwas geändert hat.
fn watchdog(app: AppHandle) {
    let mut last_tick = now_secs();
    loop {
        std::thread::sleep(Duration::from_secs(2));
        // Große Zeitsprünge zwischen zwei Durchläufen bedeuten Standby/Ruhezustand.
        let slept = now_secs().saturating_sub(last_tick) > 30;
        last_tick = now_secs();
        let state = app.state::<AppState>();
        let Some(session) = state.current() else { continue };
        let settings = state.settings();

        // Ein laufender Import/Export darf fertig werden; gesperrt wird direkt danach.
        let busy = session.busy.load(Ordering::Relaxed);
        if settings.lock_on_windows_lock && !busy && (slept || winlock::workstation_locked()) {
            state.lock();
            let _ = app.emit("vault-locked", "windows");
            continue;
        }

        let minutes = settings.auto_lock_minutes as u64;
        let idle = now_secs().saturating_sub(state.last_activity.load(Ordering::Relaxed));
        if minutes > 0 && idle > minutes * 60 && !session.busy.load(Ordering::Relaxed) {
            state.lock();
            let _ = app.emit("vault-locked", "idle");
            continue;
        }

        let due = now_secs().saturating_sub(session.last_heartbeat.load(Ordering::Relaxed)) >= lock::HEARTBEAT_SECS;
        if !due {
            continue;
        }
        session.last_heartbeat.store(now_secs(), Ordering::Relaxed);
        let mut changed = false;
        {
            let vault = session.vault.read().unwrap();
            let mut writer = session.writer.lock().unwrap();
            if let Some(w) = writer.as_mut() {
                if !matches!(w.heartbeat(&vault), Ok(true)) {
                    *writer = None;
                    *session.holder.lock().unwrap() = lock::read_lock(&vault).ok().flatten();
                    changed = true;
                }
            } else {
                // Nur lesend: wurde die Sperre freigegeben? Dann Schreibrechte übernehmen.
                if matches!(lock::read_lock(&vault), Ok(None)) {
                    if let Ok(lock::Acquire::Acquired(w)) = lock::try_acquire(&vault, false) {
                        *writer = Some(w);
                        *session.holder.lock().unwrap() = None;
                        changed = true;
                    }
                }
                let stamp = index_mtime(&vault);
                if stamp != *session.index_stamp.lock().unwrap() || changed {
                    if let Ok(idx) = vault.load_index() {
                        *session.index.write().unwrap() = idx;
                        *session.index_stamp.lock().unwrap() = stamp;
                        changed = true;
                    }
                }
            }
        }
        if changed {
            let _ = app.emit("status-changed", ());
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_asynchronous_uri_scheme_protocol("vault", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || {
                responder.respond(protocol::handle(&app, request));
            });
        })
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings: Settings = std::fs::read(&settings_path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_content_protected(settings.content_protection);
            }
            let tools = Tools::locate(app.path().resource_dir().ok());
            app.manage(AppState {
                session: Mutex::new(None),
                settings: Mutex::new(settings),
                settings_path,
                last_activity: AtomicU64::new(now_secs()),
                tools,
                cancel: Arc::new(AtomicBool::new(false)),
            });
            let handle = app.handle().clone();
            std::thread::spawn(move || watchdog(handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::status,
            commands::create_vault,
            commands::unlock,
            commands::unlock_recovery,
            commands::take_over_lock,
            commands::lock_vault,
            commands::touch,
            commands::list_models,
            commands::create_model,
            commands::rename_model,
            commands::delete_model,
            commands::set_cover,
            commands::list_media,
            commands::delete_media,
            commands::move_media,
            commands::start_import,
            commands::start_export,
            commands::cancel_job,
            commands::change_password,
            commands::save_settings,
            commands::forget_vault,
            commands::generate_password,
        ])
        .build(tauri::generate_context!())
        .expect("Fehler beim Starten der Anwendung")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    state.lock();
                }
            }
        });
}
