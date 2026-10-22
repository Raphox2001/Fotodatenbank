//! Tresor auf der Platte: Header, verschlüsselter Index und Objektablage.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::crypto::{self, Key32, KdfParams, ObjectReader};
use crate::error::{Error, Result};

pub const VAULT_FILE: &str = "vault.json";
pub const INDEX_FILE: &str = "index.enc";
pub const DEFAULT_DIR_NAME: &str = "Fotodatenbank-Tresor";
const FORMAT_NAME: &str = "fotodatenbank-tresor";
const WRAP_AAD_PASSWORD: &[u8] = b"fotodatenbank master/password v1";
const WRAP_AAD_RECOVERY: &[u8] = b"fotodatenbank master/recovery v1";
const INDEX_ID: &str = "index";

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Serialize, Deserialize, Clone)]
pub struct VaultHeader {
    pub format: String,
    pub version: u32,
    pub created: u64,
    pub kdf: KdfParams,
    pub salt: String,
    pub wrapped_by_password: String,
    pub recovery_salt: String,
    pub wrapped_by_recovery: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Video,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub created: u64,
    #[serde(default)]
    pub cover: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Media {
    pub id: String,
    pub model_id: String,
    pub kind: MediaKind,
    pub file_name: String,
    pub ext: String,
    pub size: u64,
    pub sha256: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub taken_at: Option<String>,
    pub imported_at: u64,
    pub has_thumb: bool,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Index {
    pub version: u64,
    pub models: Vec<Model>,
    pub media: Vec<Media>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ObjKind {
    Original,
    Thumb,
}

impl ObjKind {
    fn dir(self) -> &'static str {
        match self {
            ObjKind::Original => "objects",
            ObjKind::Thumb => "thumbs",
        }
    }
    fn aad_prefix(self) -> &'static str {
        match self {
            ObjKind::Original => "o:",
            ObjKind::Thumb => "t:",
        }
    }
}

pub struct Keys {
    master: Key32,
    objects: Key32,
    index: Key32,
    pub lock: Key32,
}

impl Keys {
    fn from_master(master: Key32) -> Self {
        Self {
            objects: crypto::subkey(&master, "fotodatenbank objects v1"),
            index: crypto::subkey(&master, "fotodatenbank index v1"),
            lock: crypto::subkey(&master, "fotodatenbank lock v1"),
            master,
        }
    }
}

pub struct Vault {
    root: PathBuf,
    header: VaultHeader,
    keys: Keys,
}

/// Akzeptiert den Tresor-Ordner selbst oder dessen Elternordner.
pub fn resolve_vault_dir(path: &Path) -> Option<PathBuf> {
    if path.join(VAULT_FILE).is_file() {
        return Some(path.to_path_buf());
    }
    let nested = path.join(DEFAULT_DIR_NAME);
    if nested.join(VAULT_FILE).is_file() {
        return Some(nested);
    }
    None
}

pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl Vault {
    /// Legt einen neuen Tresor an. `parent` ist der gewählte Ordner; ist er nicht leer,
    /// wird darin ein Unterordner angelegt. Gibt den Wiederherstellungsschlüssel zurück.
    pub fn create(parent: &Path, password: &str, kdf: KdfParams) -> Result<(Vault, String)> {
        if password.chars().count() < 8 {
            return Err(Error::InvalidInput("Das Passwort muss mindestens 8 Zeichen haben".into()));
        }
        if resolve_vault_dir(parent).is_some() {
            return Err(Error::InvalidInput("In diesem Ordner gibt es bereits einen Tresor".into()));
        }
        let empty = fs::read_dir(parent).map(|mut d| d.next().is_none()).unwrap_or(true);
        let root = if empty { parent.to_path_buf() } else { parent.join(DEFAULT_DIR_NAME) };
        fs::create_dir_all(root.join("objects"))?;
        fs::create_dir_all(root.join("thumbs"))?;

        let master = crypto::random_key();
        let salt = crypto::random_bytes::<16>();
        let kek = crypto::derive_password_key(password, &salt, &kdf)?;
        let recovery_raw = Zeroizing::new(crypto::random_bytes::<32>());
        let recovery_salt = crypto::random_bytes::<16>();
        let rkek = crypto::derive_recovery_key(&recovery_raw, &recovery_salt);

        let header = VaultHeader {
            format: FORMAT_NAME.into(),
            version: 1,
            created: now_secs(),
            kdf,
            salt: B64.encode(salt),
            wrapped_by_password: B64.encode(crypto::wrap_key(&kek, &master, WRAP_AAD_PASSWORD)),
            recovery_salt: B64.encode(recovery_salt),
            wrapped_by_recovery: B64.encode(crypto::wrap_key(&rkek, &master, WRAP_AAD_RECOVERY)),
        };
        let vault = Vault { root, header, keys: Keys::from_master(master) };
        vault.save_header()?;
        vault.save_index(&Index::default())?;
        Ok((vault, crypto::format_recovery(&recovery_raw)))
    }

    fn read_header(dir: &Path) -> Result<(PathBuf, VaultHeader)> {
        let root = resolve_vault_dir(dir)
            .ok_or_else(|| Error::NotFound("In diesem Ordner wurde kein Tresor gefunden".into()))?;
        let header: VaultHeader = serde_json::from_slice(&fs::read(root.join(VAULT_FILE))?)?;
        if header.format != FORMAT_NAME || header.version != 1 {
            return Err(Error::Corrupt("unbekanntes Tresor-Format".into()));
        }
        Ok((root, header))
    }

    pub fn open_with_password(dir: &Path, password: &str) -> Result<Vault> {
        let (root, header) = Self::read_header(dir)?;
        let salt = b64(&header.salt)?;
        let kek = crypto::derive_password_key(password, &salt, &header.kdf)?;
        let master = crypto::unwrap_key(&kek, &b64(&header.wrapped_by_password)?, WRAP_AAD_PASSWORD)?;
        Ok(Vault { root, header, keys: Keys::from_master(master) })
    }

    /// Öffnet mit dem Wiederherstellungsschlüssel und setzt ein neues Passwort.
    pub fn open_with_recovery(dir: &Path, recovery: &str, new_password: &str) -> Result<Vault> {
        let (root, header) = Self::read_header(dir)?;
        let raw = crypto::parse_recovery(recovery)?;
        let rkek = crypto::derive_recovery_key(&raw, &b64(&header.recovery_salt)?);
        let master = crypto::unwrap_key(&rkek, &b64(&header.wrapped_by_recovery)?, WRAP_AAD_RECOVERY)?;
        let mut vault = Vault { root, header, keys: Keys::from_master(master) };
        vault.set_password(new_password)?;
        Ok(vault)
    }

    pub fn change_password(&mut self, old: &str, new: &str) -> Result<()> {
        // altes Passwort prüfen
        let kek = crypto::derive_password_key(old, &b64(&self.header.salt)?, &self.header.kdf)?;
        crypto::unwrap_key(&kek, &b64(&self.header.wrapped_by_password)?, WRAP_AAD_PASSWORD)?;
        self.set_password(new)
    }

    fn set_password(&mut self, new: &str) -> Result<()> {
        if new.chars().count() < 8 {
            return Err(Error::InvalidInput("Das Passwort muss mindestens 8 Zeichen haben".into()));
        }
        let salt = crypto::random_bytes::<16>();
        let kek = crypto::derive_password_key(new, &salt, &self.header.kdf)?;
        self.header.salt = B64.encode(salt);
        self.header.wrapped_by_password =
            B64.encode(crypto::wrap_key(&kek, &self.keys.master, WRAP_AAD_PASSWORD));
        self.save_header()
    }

    fn save_header(&self) -> Result<()> {
        write_atomic(&self.root.join(VAULT_FILE), &serde_json::to_vec_pretty(&self.header)?)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    pub fn index_path(&self) -> PathBuf {
        self.root.join(INDEX_FILE)
    }

    pub fn load_index(&self) -> Result<Index> {
        let blob = fs::read(self.index_path())?;
        let plain = crypto::decrypt_bytes(&self.keys.index, INDEX_ID, &blob)?;
        Ok(serde_json::from_slice(&plain)?)
    }

    /// Schreibt den Index atomar und behält die vorherige Fassung als `.bak`.
    pub fn save_index(&self, index: &Index) -> Result<()> {
        let plain = Zeroizing::new(serde_json::to_vec(index)?);
        let blob = crypto::encrypt_bytes(&self.keys.index, INDEX_ID, &plain);
        let path = self.index_path();
        if path.exists() {
            let _ = fs::copy(&path, self.root.join("index.enc.bak"));
        }
        write_atomic(&path, &blob)
    }

    pub fn object_path(&self, id: &str, kind: ObjKind) -> Result<PathBuf> {
        if !valid_id(id) {
            return Err(Error::InvalidInput("ungültige ID".into()));
        }
        Ok(self.root.join(kind.dir()).join(&id[..2]).join(format!("{id}.bin")))
    }

    fn aad_id(id: &str, kind: ObjKind) -> String {
        format!("{}{}", kind.aad_prefix(), id)
    }

    /// Verschlüsselt eine Quelldatei als Original. Liefert (Größe, SHA-256 hex).
    /// Danach wird das Objekt vollständig zurückgelesen und der Hash verglichen.
    pub fn store_original(&self, id: &str, source: &Path) -> Result<(u64, String)> {
        let dest = self.object_path(id, ObjKind::Original)?;
        fs::create_dir_all(dest.parent().unwrap())?;
        let part = dest.with_extension("part");
        let file = File::open(source)?;
        let len = file.metadata()?.len();
        let digest = {
            let mut r = BufReader::with_capacity(1 << 20, file);
            let mut w = BufWriter::with_capacity(1 << 20, File::create(&part)?);
            let d = crypto::encrypt_stream(&self.keys.objects, &Self::aad_id(id, ObjKind::Original), len, &mut r, &mut w);
            let d = match d {
                Ok(d) => d,
                Err(e) => {
                    drop(w);
                    let _ = fs::remove_file(&part);
                    return Err(e);
                }
            };
            w.flush()?;
            w.into_inner().map_err(|e| Error::Io(e.to_string()))?.sync_all()?;
            d
        };
        let expected = hex::encode(digest);
        let check = (|| -> Result<String> {
            let mut reader = ObjectReader::new(BufReader::new(File::open(&part)?), &self.keys.objects, &Self::aad_id(id, ObjKind::Original))?;
            hash_reader(&mut reader)
        })();
        match check {
            Ok(h) if h == expected => {}
            Ok(_) | Err(_) => {
                let _ = fs::remove_file(&part);
                return Err(Error::Corrupt("Prüfung nach dem Verschlüsseln fehlgeschlagen".into()));
            }
        }
        fs::rename(&part, &dest)?;
        Ok((len, expected))
    }

    pub fn store_bytes(&self, id: &str, kind: ObjKind, data: &[u8]) -> Result<()> {
        let dest = self.object_path(id, kind)?;
        fs::create_dir_all(dest.parent().unwrap())?;
        let blob = crypto::encrypt_bytes(&self.keys.objects, &Self::aad_id(id, kind), data);
        write_atomic(&dest, &blob)
    }

    pub fn read_all(&self, id: &str, kind: ObjKind) -> Result<Zeroizing<Vec<u8>>> {
        let blob = fs::read(self.object_path(id, kind)?)?;
        crypto::decrypt_bytes(&self.keys.objects, &Self::aad_id(id, kind), &blob)
    }

    pub fn open_reader(&self, id: &str, kind: ObjKind) -> Result<ObjectReader<BufReader<File>>> {
        let f = File::open(self.object_path(id, kind)?)
            .map_err(|_| Error::NotFound("Datei im Tresor fehlt".into()))?;
        ObjectReader::new(BufReader::with_capacity(1 << 20, f), &self.keys.objects, &Self::aad_id(id, kind))
    }

    pub fn delete_objects(&self, id: &str) {
        for kind in [ObjKind::Original, ObjKind::Thumb] {
            if let Ok(p) = self.object_path(id, kind) {
                let _ = fs::remove_file(p);
            }
        }
    }
}

pub fn hash_reader<R: Read>(r: &mut R) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut buf = Zeroizing::new(vec![0u8; 1 << 20]);
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn b64(s: &str) -> Result<Vec<u8>> {
    B64.decode(s).map_err(|_| Error::Corrupt("Tresor-Header beschädigt".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fdb-test-{name}-{}", crypto::random_id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn create_open_and_passwords() {
        let dir = tmpdir("vault");
        let (v, recovery) = Vault::create(&dir, "sehr-geheim", KdfParams::fast_for_tests()).unwrap();
        let mut idx = v.load_index().unwrap();
        idx.models.push(Model { id: crypto::random_id(), name: "Anna".into(), created: 1, cover: None });
        v.save_index(&idx).unwrap();
        drop(v);

        assert!(matches!(Vault::open_with_password(&dir, "falsch-falsch"), Err(Error::WrongPassword)));
        let mut v = Vault::open_with_password(&dir, "sehr-geheim").unwrap();
        assert_eq!(v.load_index().unwrap().models[0].name, "Anna");

        v.change_password("sehr-geheim", "neues-passwort").unwrap();
        assert!(Vault::open_with_password(&dir, "sehr-geheim").is_err());
        Vault::open_with_password(&dir, "neues-passwort").unwrap();

        let v = Vault::open_with_recovery(&dir, &recovery, "nach-recovery").unwrap();
        assert_eq!(v.load_index().unwrap().models.len(), 1);
        Vault::open_with_password(&dir, "nach-recovery").unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn originals_are_encrypted_and_lossless() {
        let dir = tmpdir("obj");
        let (v, _) = Vault::create(&dir, "sehr-geheim", KdfParams::fast_for_tests()).unwrap();
        let src = dir.join("..").join(format!("src-{}.jpg", crypto::random_id()));
        let mut data = vec![0xFF, 0xD8, 0xFF, 0xE0];
        data.extend((0..3_000_000u32).map(|i| (i % 249) as u8));
        data.extend_from_slice(b"Anna Musterfrau");
        fs::write(&src, &data).unwrap();

        let id = crypto::random_id();
        let (len, sha) = v.store_original(&id, &src).unwrap();
        assert_eq!(len, data.len() as u64);
        assert_eq!(sha, crypto::sha256_hex(&data));
        assert_eq!(&v.read_all(&id, ObjKind::Original).unwrap()[..], &data[..]);

        let raw = fs::read(v.object_path(&id, ObjKind::Original).unwrap()).unwrap();
        assert!(!raw.starts_with(&[0xFF, 0xD8, 0xFF]));
        assert!(!raw.windows(12).any(|w| w == &data[..12]));
        assert!(!raw.windows(15).any(|w| w == b"Anna Musterfrau"));
        fs::remove_file(src).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
}
