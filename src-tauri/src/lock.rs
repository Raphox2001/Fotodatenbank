//! Schreibsperre für den Mehr-PC-Betrieb. Nur ein PC darf schreiben, alle anderen
//! öffnen den Tresor nur lesend. Der Inhalt der Sperrdatei ist verschlüsselt.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::crypto;
use crate::error::Result;
use crate::vault::{now_secs, write_atomic, Vault};

pub const LOCK_FILE: &str = "writer.lock";
pub const HEARTBEAT_SECS: u64 = 30;
pub const STALE_SECS: u64 = 120;
const LOCK_AAD: &[u8] = b"fotodatenbank writer lock v1";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LockInfo {
    pub session: String,
    pub host: String,
    pub since: u64,
    pub heartbeat: u64,
}

impl LockInfo {
    pub fn is_stale(&self) -> bool {
        now_secs().saturating_sub(self.heartbeat) > STALE_SECS
    }
}

#[derive(Debug)]
pub enum Acquire {
    Acquired(WriterLock),
    HeldBy(LockInfo),
}

#[derive(Debug)]
pub struct WriterLock {
    info: LockInfo,
    path: PathBuf,
}

pub fn hostname() -> String {
    gethostname::gethostname().to_string_lossy().into_owned()
}

fn lock_path(vault: &Vault) -> PathBuf {
    vault.root().join(LOCK_FILE)
}

fn encode(vault: &Vault, info: &LockInfo) -> Result<Vec<u8>> {
    Ok(crypto::seal(&vault.keys().lock, &serde_json::to_vec(info)?, LOCK_AAD))
}

/// Liest die aktuelle Sperre. `Ok(None)` = keine Sperre vorhanden.
/// Eine unlesbare Sperrdatei wird als verwaist behandelt.
pub fn read_lock(vault: &Vault) -> Result<Option<LockInfo>> {
    let path = lock_path(vault);
    let blob = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let info = crypto::open(&vault.keys().lock, &blob, LOCK_AAD)
        .ok()
        .and_then(|p| serde_json::from_slice::<LockInfo>(&p).ok())
        .unwrap_or(LockInfo { session: String::new(), host: "unbekannt".into(), since: 0, heartbeat: 0 });
    Ok(Some(info))
}

/// Versucht die Schreibsperre zu bekommen. Mit `force` wird eine verwaiste Sperre
/// übernommen. Eine verwaiste Sperre desselben PCs wird immer übernommen.
pub fn try_acquire(vault: &Vault, force: bool) -> Result<Acquire> {
    let path = lock_path(vault);
    let host = hostname();
    if let Some(existing) = read_lock(vault)? {
        let takeover = existing.is_stale() && (force || existing.host == host);
        if !takeover {
            return Ok(Acquire::HeldBy(existing));
        }
        let _ = fs::remove_file(&path);
    }
    let now = now_secs();
    let info = LockInfo { session: crypto::random_id(), host, since: now, heartbeat: now };
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut f) => {
            f.write_all(&encode(vault, &info)?)?;
            f.sync_all()?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Ein anderer PC war schneller.
            return Ok(match read_lock(vault)? {
                Some(other) => Acquire::HeldBy(other),
                None => Acquire::HeldBy(info),
            });
        }
        Err(e) => return Err(e.into()),
    }
    let lock = WriterLock { info, path };
    if !lock.still_ours(vault) {
        return Ok(Acquire::HeldBy(read_lock(vault)?.unwrap_or(lock.info)));
    }
    Ok(Acquire::Acquired(lock))
}

impl WriterLock {
    pub fn still_ours(&self, vault: &Vault) -> bool {
        matches!(read_lock(vault), Ok(Some(i)) if i.session == self.info.session)
    }

    /// Aktualisiert den Zeitstempel. `false`, wenn die Sperre verloren ging.
    pub fn heartbeat(&mut self, vault: &Vault) -> Result<bool> {
        if !self.still_ours(vault) {
            return Ok(false);
        }
        self.info.heartbeat = now_secs();
        write_atomic(&self.path, &encode(vault, &self.info)?)?;
        Ok(true)
    }

    pub fn release(&self, vault: &Vault) {
        if self.still_ours(vault) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KdfParams;

    #[test]
    fn second_opener_is_read_only_until_released() {
        let dir = std::env::temp_dir().join(format!("fdb-lock-{}", crypto::random_id()));
        fs::create_dir_all(&dir).unwrap();
        let (a, _) = Vault::create(&dir, "sehr-geheim", KdfParams::fast_for_tests()).unwrap();
        let b = Vault::open_with_password(&dir, "sehr-geheim").unwrap();

        let Acquire::Acquired(mut la) = try_acquire(&a, false).unwrap() else { panic!() };
        assert!(matches!(try_acquire(&b, true).unwrap(), Acquire::HeldBy(_)));
        assert!(la.heartbeat(&a).unwrap());

        // Sperre künstlich veralten lassen -> mit force übernehmbar
        la.info.heartbeat = 0;
        write_atomic(&la.path, &encode(&a, &la.info).unwrap()).unwrap();
        let Acquire::Acquired(lb) = try_acquire(&b, true).unwrap() else { panic!() };
        assert!(!la.heartbeat(&a).unwrap(), "A muss den Verlust bemerken");
        lb.release(&b);
        assert!(read_lock(&a).unwrap().is_none());
        fs::remove_dir_all(dir).unwrap();
    }
}
