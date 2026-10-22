//! Verschlüsselter Export als ZIP (AES-256) oder 7z (AES-256, auch Dateinamen verschlüsselt).
//! Die Daten werden direkt aus dem Tresor entschlüsselt ins Archiv geschrieben,
//! ohne unverschlüsselte Zwischendateien. Danach wird das Archiv geprüft.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::vault::{hash_reader, Index, Media, MediaKind, ObjKind, Vault};

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Zip,
    #[serde(rename = "7z")]
    SevenZ,
}

pub const MIN_PASSWORD_LEN: usize = 10;

pub struct Entry {
    pub path: String,
    pub media: Media,
}

/// Ungültige Zeichen für Windows-Dateinamen ersetzen.
pub fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').trim().to_string();
    if trimmed.is_empty() {
        "Unbenannt".into()
    } else {
        trimmed
    }
}

fn unique(path: String, used: &mut HashSet<String>) -> String {
    if used.insert(path.to_lowercase()) {
        return path;
    }
    let (stem, ext) = match path.rfind('.') {
        Some(i) if i > path.rfind('/').map_or(0, |s| s + 1) => (&path[..i], &path[i..]),
        _ => (path.as_str(), ""),
    };
    (2..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|p| used.insert(p.to_lowercase()))
        .unwrap()
}

/// Ordnerstruktur: `<Model>/Fotos/<Datei>` und `<Model>/Videos/<Datei>`.
pub fn plan(index: &Index, model_ids: &[String]) -> Vec<Entry> {
    let mut models: Vec<_> = index
        .models
        .iter()
        .filter(|m| model_ids.is_empty() || model_ids.contains(&m.id))
        .collect();
    models.sort_by_key(|m| m.name.to_lowercase());
    let mut used_dirs = HashSet::new();
    let mut used = HashSet::new();
    let mut out = Vec::new();
    for m in models {
        let dir = unique(sanitize(&m.name), &mut used_dirs);
        let mut media: Vec<_> = index.media.iter().filter(|x| x.model_id == m.id).collect();
        media.sort_by(|a, b| (&a.taken_at, a.imported_at, &a.file_name).cmp(&(&b.taken_at, b.imported_at, &b.file_name)));
        for x in media {
            let sub = match x.kind {
                MediaKind::Image => "Fotos",
                MediaKind::Video => "Videos",
            };
            let path = unique(format!("{dir}/{sub}/{}", sanitize(&x.file_name)), &mut used);
            out.push(Entry { path, media: x.clone() });
        }
    }
    out
}

pub struct Progress<'a> {
    pub cancel: &'a AtomicBool,
    pub report: &'a dyn Fn(&str, u64, u64),
}

/// Reicht gelesene Bytes durch und meldet den Fortschritt.
struct Counting<'a, R: Read> {
    inner: R,
    done: &'a mut u64,
    total: u64,
    progress: &'a Progress<'a>,
    phase: &'a str,
    last: u64,
}

impl<R: Read> Read for Counting<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.progress.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "abgebrochen"));
        }
        let n = self.inner.read(buf)?;
        *self.done += n as u64;
        if *self.done - self.last > 8 * 1024 * 1024 || n == 0 {
            self.last = *self.done;
            (self.progress.report)(self.phase, *self.done, self.total);
        }
        Ok(n)
    }
}

pub fn export(
    vault: &Vault,
    entries: &[Entry],
    format: Format,
    password: &str,
    dest: &Path,
    progress: &Progress,
) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(Error::InvalidInput(format!(
            "Das Archiv-Passwort muss mindestens {MIN_PASSWORD_LEN} Zeichen haben"
        )));
    }
    if entries.is_empty() {
        return Err(Error::InvalidInput("Es gibt nichts zu exportieren".into()));
    }
    let part = PathBuf::from(format!("{}.part", dest.display()));
    let result = write_archive(vault, entries, format, password, &part, progress)
        .and_then(|_| verify(&part, entries, format, password, progress));
    match result {
        Ok(()) => {
            if dest.exists() {
                fs::remove_file(dest)?;
            }
            fs::rename(&part, dest)?;
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&part);
            Err(if progress.cancel.load(Ordering::Relaxed) {
                Error::Other("Export abgebrochen".into())
            } else {
                e
            })
        }
    }
}

fn write_archive(
    vault: &Vault,
    entries: &[Entry],
    format: Format,
    password: &str,
    path: &Path,
    progress: &Progress,
) -> Result<()> {
    let total: u64 = entries.iter().map(|e| e.media.size).sum();
    let mut done = 0u64;
    match format {
        Format::Zip => {
            let mut zip = zip::ZipWriter::new(BufWriter::with_capacity(1 << 20, File::create(path)?));
            for e in entries {
                let opts = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .large_file(e.media.size >= u32::MAX as u64)
                    .with_aes_encryption(zip::AesMode::Aes256, password);
                zip.start_file(e.path.as_str(), opts).map_err(zip_err)?;
                let reader = vault.open_reader(&e.media.id, ObjKind::Original)?;
                let mut r = Counting { inner: reader, done: &mut done, total, progress, phase: "write", last: 0 };
                io::copy(&mut r, &mut zip)?;
            }
            let mut w = zip.finish().map_err(zip_err)?;
            w.flush()?;
            w.into_inner().map_err(|e| Error::Io(e.to_string()))?.sync_all()?;
        }
        Format::SevenZ => {
            use sevenz_rust2::encoder_options::AesEncoderOptions;
            use sevenz_rust2::{ArchiveEntry, ArchiveWriter, EncoderMethod, Password};
            let mut sz = ArchiveWriter::new(BufWriter::with_capacity(1 << 20, File::create(path)?)).map_err(sz_err)?;
            sz.set_content_methods(vec![
                AesEncoderOptions::new(Password::new(password)).into(),
                EncoderMethod::COPY.into(),
            ]);
            sz.set_encrypt_header(true);
            for e in entries {
                let reader = vault.open_reader(&e.media.id, ObjKind::Original)?;
                let r = Counting { inner: reader, done: &mut done, total, progress, phase: "write", last: 0 };
                sz.push_archive_entry(ArchiveEntry::new_file(&e.path), Some(r)).map_err(sz_err)?;
            }
            let mut w = sz.finish()?;
            w.flush()?;
            w.into_inner().map_err(|e| Error::Io(e.to_string()))?.sync_all()?;
        }
    }
    Ok(())
}

/// Liest das fertige Archiv mit dem Passwort zurück und vergleicht jede Datei per SHA-256.
fn verify(path: &Path, entries: &[Entry], format: Format, password: &str, progress: &Progress) -> Result<()> {
    let total: u64 = entries.iter().map(|e| e.media.size).sum();
    let mut done = 0u64;
    let fail = |name: &str| Error::Corrupt(format!("Prüfung des Archivs fehlgeschlagen: {name}"));
    match format {
        Format::Zip => {
            let mut zip = zip::ZipArchive::new(BufReader::new(File::open(path)?)).map_err(zip_err)?;
            if zip.len() != entries.len() {
                return Err(fail("Anzahl der Dateien"));
            }
            for (i, e) in entries.iter().enumerate() {
                let f = zip.by_index_decrypt(i, password.as_bytes()).map_err(zip_err)?;
                if f.name() != e.path {
                    return Err(fail(&e.path));
                }
                let mut r = Counting { inner: f, done: &mut done, total, progress, phase: "verify", last: 0 };
                if hash_reader(&mut r)? != e.media.sha256 {
                    return Err(fail(&e.path));
                }
            }
        }
        Format::SevenZ => {
            use sevenz_rust2::{ArchiveReader, Password};
            let mut sz = ArchiveReader::new(BufReader::new(File::open(path)?), Password::new(password)).map_err(sz_err)?;
            let expected: std::collections::HashMap<&str, &str> =
                entries.iter().map(|e| (e.path.as_str(), e.media.sha256.as_str())).collect();
            let mut seen = 0usize;
            let mut bad: Option<String> = None;
            sz.for_each_entries(|entry, reader| {
                if entry.is_directory() {
                    return Ok(true);
                }
                let mut r = Counting { inner: reader, done: &mut done, total, progress, phase: "verify", last: 0 };
                let h = hash_reader(&mut r).map_err(|e| sevenz_rust2::Error::Other(e.to_string().into()))?;
                seen += 1;
                if expected.get(entry.name()) != Some(&h.as_str()) {
                    bad = Some(entry.name().to_string());
                    return Ok(false);
                }
                Ok(true)
            })
            .map_err(sz_err)?;
            if let Some(name) = bad {
                return Err(fail(&name));
            }
            if seen != entries.len() {
                return Err(fail("Anzahl der Dateien"));
            }
        }
    }
    Ok(())
}

fn zip_err(e: zip::result::ZipError) -> Error {
    match e {
        zip::result::ZipError::Io(io) => io.into(),
        other => Error::Other(format!("ZIP: {other}")),
    }
}

fn sz_err(e: sevenz_rust2::Error) -> Error {
    Error::Other(format!("7z: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{self, KdfParams};
    use crate::vault::Model;

    fn setup() -> (PathBuf, Vault, Index) {
        let dir = std::env::temp_dir().join(format!("fdb-export-{}", crypto::random_id()));
        fs::create_dir_all(&dir).unwrap();
        let (v, _) = Vault::create(&dir, "sehr-geheim", KdfParams::fast_for_tests()).unwrap();
        let mut idx = Index::default();
        let model = Model { id: crypto::random_id(), name: "Anna: Shooting/2024".into(), created: 0, cover: None };
        for (i, name) in ["bild.jpg", "bild.jpg", "clip.mp4"].iter().enumerate() {
            let src = dir.join(format!("src{i}"));
            let data: Vec<u8> = (0..(1_500_000 + i * 7)).map(|j| ((j * (i + 3)) % 256) as u8).collect();
            fs::write(&src, &data).unwrap();
            let id = crypto::random_id();
            let (size, sha256) = v.store_original(&id, &src).unwrap();
            idx.media.push(Media {
                id,
                model_id: model.id.clone(),
                kind: if name.ends_with("mp4") { MediaKind::Video } else { MediaKind::Image },
                file_name: name.to_string(),
                ext: "x".into(),
                size,
                sha256,
                width: None,
                height: None,
                duration: None,
                taken_at: None,
                imported_at: i as u64,
                has_thumb: false,
            });
        }
        idx.models.push(model);
        (dir, v, idx)
    }

    #[test]
    fn plan_structure() {
        let (dir, _v, idx) = setup();
        let paths: Vec<_> = plan(&idx, &[]).into_iter().map(|e| e.path).collect();
        assert_eq!(paths, vec![
            "Anna_ Shooting_2024/Fotos/bild.jpg",
            "Anna_ Shooting_2024/Fotos/bild (2).jpg",
            "Anna_ Shooting_2024/Videos/clip.mp4",
        ]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn zip_and_7z_roundtrip() {
        let (dir, v, idx) = setup();
        let entries = plan(&idx, &[]);
        let cancel = AtomicBool::new(false);
        let progress = Progress { cancel: &cancel, report: &|_, _, _| {} };
        for (format, file) in [(Format::Zip, "out.zip"), (Format::SevenZ, "out.7z")] {
            let dest = dir.join(file);
            export(&v, &entries, format, "archiv-passwort-123", &dest, &progress).unwrap();
            assert!(dest.exists());
            // falsches Passwort muss scheitern
            assert!(verify(&dest, &entries, format, "falsches-passwort", &progress).is_err());
            let raw = fs::read(&dest).unwrap();
            if format == Format::SevenZ {
                assert!(!raw.windows(4).any(|w| w == b"Anna"), "7z darf keine Namen zeigen");
            }
        }
        assert!(export(&v, &entries, Format::Zip, "kurz", &dir.join("x.zip"), &progress).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
