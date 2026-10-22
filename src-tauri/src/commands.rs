//! Befehle, die die Oberfläche aufrufen kann.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use zeroize::Zeroizing;

use crate::crypto::{self, KdfParams};
use crate::error::{Error, Result};
use crate::export::{self, Format};
use crate::lock::{self, Acquire};
use crate::vault::{self, now_secs, Media, MediaKind, Model, Vault};
use crate::{import, AppState, Session, Settings};

#[derive(Serialize)]
pub struct AppInfo {
    settings: Settings,
    vault_found: bool,
    hostname: String,
    ffmpeg: bool,
}

#[derive(Serialize)]
pub struct Holder {
    host: String,
    since: u64,
    stale: bool,
}

#[derive(Serialize)]
pub struct Status {
    unlocked: bool,
    read_only: bool,
    holder: Option<Holder>,
    vault_path: Option<String>,
    busy: bool,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    let settings = state.settings();
    let vault_found = settings
        .vault_path
        .as_ref()
        .is_some_and(|p| vault::resolve_vault_dir(&PathBuf::from(p)).is_some());
    AppInfo { settings, vault_found, hostname: lock::hostname(), ffmpeg: state.tools.ffmpeg.is_some() }
}

#[tauri::command]
pub fn status(state: State<'_, AppState>) -> Status {
    let Some(s) = state.current() else {
        return Status { unlocked: false, read_only: false, holder: None, vault_path: state.settings().vault_path, busy: false };
    };
    let holder = s.holder.lock().unwrap().as_ref().map(|h| Holder { host: h.host.clone(), since: h.since, stale: h.is_stale() });
    let vault_path = Some(s.vault.read().unwrap().root().display().to_string());
    Status { unlocked: true, read_only: s.read_only(), holder, vault_path, busy: s.busy.load(Ordering::Relaxed) }
}

fn remember_path(state: &AppState, vault: &Vault) -> Result<()> {
    let mut settings = state.settings();
    settings.vault_path = Some(vault.root().display().to_string());
    state.save_settings(settings)
}

fn start_session(state: &AppState, vault: Vault, force: bool) -> Result<()> {
    let index = vault.load_index()?;
    let (writer, holder) = match lock::try_acquire(&vault, force)? {
        Acquire::Acquired(w) => (Some(w), None),
        Acquire::HeldBy(info) => (None, Some(info)),
    };
    remember_path(state, &vault)?;
    state.set_session(Session::new(vault, index, writer, holder));
    Ok(())
}

#[derive(Serialize)]
pub struct Created {
    recovery_key: String,
    vault_path: String,
}

#[tauri::command(async)]
pub fn create_vault(state: State<'_, AppState>, parent: String, password: String) -> Result<Created> {
    let password = Zeroizing::new(password);
    let (vault, recovery_key) = Vault::create(&PathBuf::from(parent), &password, KdfParams::strong())?;
    let vault_path = vault.root().display().to_string();
    start_session(&state, vault, false)?;
    Ok(Created { recovery_key, vault_path })
}

fn vault_dir(state: &AppState, path: Option<String>) -> Result<PathBuf> {
    path.or_else(|| state.settings().vault_path)
        .map(PathBuf::from)
        .ok_or_else(|| Error::InvalidInput("Bitte zuerst einen Tresor-Ordner wählen".into()))
}

#[tauri::command(async)]
pub fn unlock(state: State<'_, AppState>, path: Option<String>, password: String) -> Result<Status> {
    let password = Zeroizing::new(password);
    let vault = Vault::open_with_password(&vault_dir(&state, path)?, &password)?;
    start_session(&state, vault, false)?;
    Ok(status(state))
}

#[tauri::command(async)]
pub fn unlock_recovery(state: State<'_, AppState>, path: Option<String>, recovery_key: String, new_password: String) -> Result<Status> {
    let recovery_key = Zeroizing::new(recovery_key);
    let new_password = Zeroizing::new(new_password);
    let vault = Vault::open_with_recovery(&vault_dir(&state, path)?, &recovery_key, &new_password)?;
    start_session(&state, vault, false)?;
    Ok(status(state))
}

/// Übernimmt eine verwaiste Schreibsperre (anderer PC abgestürzt o. Ä.).
#[tauri::command(async)]
pub fn take_over_lock(state: State<'_, AppState>) -> Result<Status> {
    let s = state.session()?;
    {
        let vault = s.vault.read().unwrap();
        match lock::try_acquire(&vault, true)? {
            Acquire::Acquired(w) => {
                let idx = vault.load_index()?;
                *s.index.write().unwrap() = idx;
                *s.writer.lock().unwrap() = Some(w);
                *s.holder.lock().unwrap() = None;
            }
            Acquire::HeldBy(info) => {
                let msg = format!("Der Tresor wird noch aktiv an „{}“ verwendet", info.host);
                *s.holder.lock().unwrap() = Some(info);
                return Err(Error::Other(msg));
            }
        }
    }
    Ok(status(state))
}

#[tauri::command]
pub fn lock_vault(state: State<'_, AppState>) {
    state.lock();
}

#[tauri::command]
pub fn touch(state: State<'_, AppState>) {
    state.touch();
}

#[derive(Serialize)]
pub struct ModelInfo {
    id: String,
    name: String,
    created: u64,
    cover: Option<String>,
    images: usize,
    videos: usize,
    bytes: u64,
}

fn model_info(m: &Model, media: &[Media]) -> ModelInfo {
    let mine = media.iter().filter(|x| x.model_id == m.id);
    let (mut images, mut videos, mut bytes) = (0, 0, 0);
    for x in mine {
        match x.kind {
            MediaKind::Image => images += 1,
            MediaKind::Video => videos += 1,
        }
        bytes += x.size;
    }
    ModelInfo { id: m.id.clone(), name: m.name.clone(), created: m.created, cover: m.cover.clone(), images, videos, bytes }
}

#[tauri::command]
pub fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelInfo>> {
    state.touch();
    let s = state.session()?;
    let idx = s.index.read().unwrap();
    let mut out: Vec<_> = idx.models.iter().map(|m| model_info(m, &idx.media)).collect();
    out.sort_by_key(|m| m.name.to_lowercase());
    Ok(out)
}

fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::InvalidInput("Bitte einen Namen eingeben".into()));
    }
    Ok(name.chars().take(120).collect())
}

#[tauri::command(async)]
pub fn create_model(state: State<'_, AppState>, name: String) -> Result<ModelInfo> {
    state.touch();
    let name = clean_name(&name)?;
    let s = state.session()?;
    s.mutate(|idx| {
        let m = Model { id: crypto::random_id(), name, created: now_secs(), cover: None };
        idx.models.push(m.clone());
        Ok(model_info(&m, &[]))
    })
}

#[tauri::command(async)]
pub fn rename_model(state: State<'_, AppState>, id: String, name: String) -> Result<()> {
    state.touch();
    let name = clean_name(&name)?;
    state.session()?.mutate(|idx| {
        let m = idx.models.iter_mut().find(|m| m.id == id).ok_or_else(|| Error::NotFound("Model".into()))?;
        m.name = name;
        Ok(())
    })
}

#[tauri::command(async)]
pub fn delete_model(state: State<'_, AppState>, id: String) -> Result<()> {
    state.touch();
    let s = state.session()?;
    let removed = s.mutate(|idx| {
        idx.models.retain(|m| m.id != id);
        let (gone, keep): (Vec<_>, Vec<_>) = idx.media.drain(..).partition(|m| m.model_id == id);
        idx.media = keep;
        Ok(gone)
    })?;
    let vault = s.vault.read().unwrap();
    for m in removed {
        vault.delete_objects(&m.id);
    }
    Ok(())
}

#[tauri::command(async)]
pub fn set_cover(state: State<'_, AppState>, model_id: String, media_id: String) -> Result<()> {
    state.touch();
    state.session()?.mutate(|idx| {
        let m = idx.models.iter_mut().find(|m| m.id == model_id).ok_or_else(|| Error::NotFound("Model".into()))?;
        m.cover = Some(media_id);
        Ok(())
    })
}

#[tauri::command]
pub fn list_media(state: State<'_, AppState>, model_id: String) -> Result<Vec<Media>> {
    state.touch();
    let s = state.session()?;
    let idx = s.index.read().unwrap();
    Ok(idx.media.iter().filter(|m| m.model_id == model_id).cloned().collect())
}

#[tauri::command(async)]
pub fn delete_media(state: State<'_, AppState>, ids: Vec<String>) -> Result<()> {
    state.touch();
    let s = state.session()?;
    s.mutate(|idx| {
        idx.media.retain(|m| !ids.contains(&m.id));
        for model in idx.models.iter_mut() {
            if model.cover.as_ref().is_some_and(|c| ids.contains(c)) {
                model.cover = idx.media.iter().find(|x| x.model_id == model.id && x.has_thumb).map(|x| x.id.clone());
            }
        }
        Ok(())
    })?;
    let vault = s.vault.read().unwrap();
    for id in &ids {
        vault.delete_objects(id);
    }
    Ok(())
}

#[tauri::command(async)]
pub fn move_media(state: State<'_, AppState>, ids: Vec<String>, target_model_id: String) -> Result<()> {
    state.touch();
    state.session()?.mutate(|idx| {
        if !idx.models.iter().any(|m| m.id == target_model_id) {
            return Err(Error::NotFound("Ziel-Model".into()));
        }
        for m in idx.media.iter_mut().filter(|m| ids.contains(&m.id)) {
            m.model_id = target_model_id.clone();
        }
        for model in idx.models.iter_mut() {
            let stale = model.cover.as_ref().is_some_and(|c| idx.media.iter().any(|x| &x.id == c && x.model_id != model.id));
            if stale || model.cover.is_none() {
                model.cover = idx.media.iter().find(|x| x.model_id == model.id && x.has_thumb).map(|x| x.id.clone());
            }
        }
        Ok(())
    })
}

fn begin_job(state: &AppState) -> Result<std::sync::Arc<Session>> {
    let s = state.session()?;
    if s.read_only() {
        return Err(Error::ReadOnly);
    }
    if s.busy.swap(true, Ordering::SeqCst) {
        return Err(Error::Other("Es läuft bereits ein Import oder Export".into()));
    }
    state.cancel.store(false, Ordering::Relaxed);
    Ok(s)
}

#[tauri::command]
pub fn start_import(app: AppHandle, state: State<'_, AppState>, model_id: String, paths: Vec<String>) -> Result<()> {
    state.touch();
    let s = begin_job(&state)?;
    let tools = state.tools.clone();
    let cancel = state.cancel.clone();
    let paths = paths.into_iter().map(PathBuf::from).collect();
    std::thread::spawn(move || import::run(app, s, tools, cancel, model_id, paths));
    Ok(())
}

#[derive(Serialize, Clone)]
struct ExportProgress {
    phase: String,
    done: u64,
    total: u64,
    finished: bool,
    error: Option<String>,
    files: usize,
}

#[derive(Deserialize)]
pub struct ExportRequest {
    model_ids: Vec<String>,
    format: Format,
    password: String,
    dest: String,
}

#[tauri::command]
pub fn start_export(app: AppHandle, state: State<'_, AppState>, request: ExportRequest) -> Result<()> {
    state.touch();
    // Export ist auch im Nur-Lesen-Modus erlaubt, er ändert den Tresor nicht.
    let s = state.session()?;
    if s.busy.swap(true, Ordering::SeqCst) {
        return Err(Error::Other("Es läuft bereits ein Import oder Export".into()));
    }
    state.cancel.store(false, Ordering::Relaxed);
    let cancel = state.cancel.clone();
    std::thread::spawn(move || {
        let password = Zeroizing::new(request.password);
        let entries = export::plan(&s.index.read().unwrap(), &request.model_ids);
        let files = entries.len();
        let report = |phase: &str, done: u64, total: u64| {
            let _ = app.emit("export-progress", ExportProgress { phase: phase.into(), done, total, finished: false, error: None, files });
        };
        let progress = export::Progress { cancel: &cancel, report: &report };
        let result = {
            let vault = s.vault.read().unwrap();
            export::export(&vault, &entries, request.format, &password, &PathBuf::from(&request.dest), &progress)
        };
        s.busy.store(false, Ordering::Relaxed);
        let _ = app.emit(
            "export-progress",
            ExportProgress { phase: "done".into(), done: 0, total: 0, finished: true, error: result.err().map(|e| e.to_string()), files },
        );
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>) {
    state.cancel.store(true, Ordering::Relaxed);
}

#[tauri::command(async)]
pub fn change_password(state: State<'_, AppState>, old_password: String, new_password: String) -> Result<()> {
    state.touch();
    let old = Zeroizing::new(old_password);
    let new = Zeroizing::new(new_password);
    let s = state.session()?;
    if s.read_only() {
        return Err(Error::ReadOnly);
    }
    let mut vault = s.vault.write().unwrap();
    vault.change_password(&old, &new)
}

#[derive(Deserialize)]
pub struct SettingsUpdate {
    auto_lock_minutes: u32,
    content_protection: bool,
    lock_on_windows_lock: bool,
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<'_, AppState>, update: SettingsUpdate) -> Result<Settings> {
    state.touch();
    let mut s = state.settings();
    s.auto_lock_minutes = update.auto_lock_minutes.min(24 * 60);
    s.content_protection = update.content_protection;
    s.lock_on_windows_lock = update.lock_on_windows_lock;
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_content_protected(s.content_protection);
    }
    state.save_settings(s.clone())?;
    Ok(s)
}

/// Vergisst den gespeicherten Tresor-Pfad (um einen anderen Tresor zu öffnen).
#[tauri::command]
pub fn forget_vault(state: State<'_, AppState>) -> Result<()> {
    state.lock();
    let mut s = state.settings();
    s.vault_path = None;
    state.save_settings(s)
}

/// Starkes Zufallspasswort für Archive (ohne leicht verwechselbare Zeichen).
#[tauri::command]
pub fn generate_password() -> String {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let bytes = crypto::random_bytes::<64>();
    let chars: Vec<char> = bytes
        .iter()
        .filter(|b| (**b as usize) < 256 - 256 % CHARS.len())
        .take(20)
        .map(|b| CHARS[*b as usize % CHARS.len()] as char)
        .collect();
    chars.chunks(5).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-")
}
