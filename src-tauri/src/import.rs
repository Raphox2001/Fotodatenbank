//! Import von Dateien und Ordnern in ein Model (läuft im Hintergrund).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::error::Result;
use crate::media::{self, Tools};
use crate::vault::{now_secs, Media, MediaKind, ObjKind};
use crate::{crypto, Session};

#[derive(Serialize, Clone, Default)]
pub struct ImportProgress {
    pub model_id: String,
    pub total: usize,
    pub done: usize,
    pub imported: usize,
    pub duplicates: usize,
    pub current: String,
    pub errors: Vec<String>,
    pub unsupported: Vec<String>,
    pub finished: bool,
    pub cancelled: bool,
}

fn import_one(session: &Session, tools: &Tools, model_id: &str, path: &Path) -> Result<Media> {
    let (kind, ext) = media::classify(path).expect("vorher geprüft");
    let id = crypto::random_id();
    let vault = session.vault.read().unwrap();
    let (size, sha256) = vault.store_original(&id, path)?;

    let (mut width, mut height, mut duration, mut thumb) = (None, None, None, None);
    let taken_at;
    match kind {
        MediaKind::Image => {
            if let Ok(info) = media::image_info(path) {
                width = Some(info.width);
                height = Some(info.height);
                thumb = Some(info.thumb);
            }
            taken_at = media::exif_date(path);
        }
        MediaKind::Video => {
            let info = media::video_info(tools, path);
            width = info.width;
            height = info.height;
            duration = info.duration;
            taken_at = info.taken_at;
            thumb = media::video_thumb(tools, path, duration);
        }
    }
    let has_thumb = match &thumb {
        Some(t) => {
            if let Err(e) = vault.store_bytes(&id, ObjKind::Thumb, t) {
                vault.delete_objects(&id);
                return Err(e);
            }
            true
        }
        None => false,
    };
    Ok(Media {
        id,
        model_id: model_id.to_string(),
        kind,
        file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        ext,
        size,
        sha256,
        width,
        height,
        duration,
        taken_at,
        imported_at: now_secs(),
        has_thumb,
    })
}

pub fn run(app: AppHandle, session: Arc<Session>, tools: Tools, cancel: Arc<std::sync::atomic::AtomicBool>, model_id: String, paths: Vec<PathBuf>) {
    let mut p = ImportProgress { model_id: model_id.clone(), ..Default::default() };
    let files = media::collect_files(&paths, &mut p.unsupported);
    p.total = files.len();
    let _ = app.emit("import-progress", &p);

    let mut known: HashSet<String> = session
        .index
        .read()
        .unwrap()
        .media
        .iter()
        .filter(|m| m.model_id == model_id)
        .map(|m| m.sha256.clone())
        .collect();
    let mut pending: Vec<Media> = Vec::new();

    let flush = |pending: &mut Vec<Media>, p: &mut ImportProgress| {
        if pending.is_empty() {
            return;
        }
        let batch = std::mem::take(pending);
        let n = batch.len();
        let res = session.mutate(|idx| {
            let Some(model) = idx.models.iter_mut().find(|m| m.id == model_id) else {
                return Err(crate::error::Error::NotFound("Model wurde gelöscht".into()));
            };
            if model.cover.is_none() {
                model.cover = batch.iter().find(|m| m.has_thumb).map(|m| m.id.clone());
            }
            idx.media.extend(batch.iter().cloned());
            Ok(())
        });
        match res {
            Ok(()) => p.imported += n,
            Err(e) => {
                let vault = session.vault.read().unwrap();
                for m in &batch {
                    vault.delete_objects(&m.id);
                }
                p.errors.push(format!("{n} Dateien konnten nicht gespeichert werden: {e}"));
            }
        }
    };

    for path in &files {
        if cancel.load(Ordering::Relaxed) {
            p.cancelled = true;
            break;
        }
        p.current = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let _ = app.emit("import-progress", &p);
        match import_one(&session, &tools, &model_id, path) {
            Ok(m) => {
                if known.insert(m.sha256.clone()) {
                    pending.push(m);
                } else {
                    session.vault.read().unwrap().delete_objects(&m.id);
                    p.duplicates += 1;
                }
            }
            Err(e) => p.errors.push(format!("{}: {e}", p.current)),
        }
        p.done += 1;
        if pending.len() >= 20 {
            flush(&mut pending, &mut p);
            let _ = app.emit("status-changed", ());
        }
    }
    flush(&mut pending, &mut p);
    p.finished = true;
    p.current.clear();
    session.busy.store(false, Ordering::Relaxed);
    let _ = app.emit("import-progress", &p);
    let _ = app.emit("status-changed", ());
}
