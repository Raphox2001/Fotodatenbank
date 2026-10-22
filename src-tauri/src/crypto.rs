//! Kryptografischer Kern: Schlüsselableitung, Key-Wrapping und blockweise
//! authentifizierte Verschlüsselung von Dateien (XChaCha20-Poly1305).
//!
//! Objektformat (alle Zahlen little endian):
//! ```text
//! "FDB1" | version u8 | chunk_size u32 | nonce_prefix [16] | plain_len u64 | chunks...
//! ```
//! Jeder Block = Klartext-Block + 16 Byte Tag. Nonce = prefix ‖ blocknummer (u64).
//! AAD = Header ‖ Objekt-ID ‖ blocknummer ‖ letzter-block-flag. Dadurch werden
//! Vertauschen, Umsortieren, Abschneiden und Manipulation erkannt.

use std::io::{self, Read, Seek, SeekFrom, Write};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::error::{Error, Result};

pub const CHUNK_SIZE: u32 = 1024 * 1024;
const MAGIC: &[u8; 4] = b"FDB1";
const FORMAT_VERSION: u8 = 1;
pub const HEADER_LEN: usize = 4 + 1 + 4 + 16 + 8;
const TAG_LEN: usize = 16;

pub type Key32 = Zeroizing<[u8; 32]>;

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    buf
}

pub fn random_key() -> Key32 {
    Zeroizing::new(random_bytes::<32>())
}

/// Zufällige, nicht sprechende ID (32 Hex-Zeichen) für Objekte und Models.
pub fn random_id() -> String {
    hex::encode(random_bytes::<16>())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl KdfParams {
    /// Produktiv-Parameter: 256 MiB Speicher, 3 Durchläufe.
    pub fn strong() -> Self {
        Self { m_kib: 256 * 1024, t: 3, p: 4 }
    }

    #[cfg(test)]
    pub fn fast_for_tests() -> Self {
        Self { m_kib: 1024, t: 1, p: 1 }
    }
}

pub fn derive_password_key(password: &str, salt: &[u8], params: &KdfParams) -> Result<Key32> {
    let p = Params::new(params.m_kib, params.t, params.p, Some(32))
        .map_err(|e| Error::Crypto(format!("Argon2-Parameter: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, p);
    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password.as_bytes(), salt, out.as_mut())
        .map_err(|e| Error::Crypto(format!("Argon2: {e}")))?;
    Ok(out)
}

/// Unterschlüssel aus dem Master-Key (HKDF-SHA256).
pub fn subkey(master: &[u8; 32], info: &str) -> Key32 {
    let hk = Hkdf::<Sha256>::new(None, master);
    let mut out = Zeroizing::new([0u8; 32]);
    hk.expand(info.as_bytes(), out.as_mut())
        .expect("32 Byte sind eine gültige HKDF-Länge");
    out
}

/// Schlüssel aus dem (hoch-entropischen) Wiederherstellungsschlüssel.
pub fn derive_recovery_key(raw: &[u8; 32], salt: &[u8]) -> Key32 {
    let hk = Hkdf::<Sha256>::new(Some(salt), raw);
    let mut out = Zeroizing::new([0u8; 32]);
    hk.expand(b"fotodatenbank recovery wrap v1", out.as_mut())
        .expect("32 Byte sind eine gültige HKDF-Länge");
    out
}

fn cipher(key: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(key.into())
}

/// Verpackt einen Schlüssel: nonce(24) ‖ ciphertext(32+16).
pub fn wrap_key(kek: &[u8; 32], key: &[u8; 32], aad: &[u8]) -> Vec<u8> {
    seal(kek, key, aad)
}

pub fn unwrap_key(kek: &[u8; 32], blob: &[u8], aad: &[u8]) -> Result<Key32> {
    let plain = open(kek, blob, aad)?;
    if plain.len() != 32 {
        return Err(Error::WrongPassword);
    }
    let mut out = Zeroizing::new([0u8; 32]);
    out.copy_from_slice(&plain);
    Ok(out)
}

/// Kleine Nachricht verschlüsseln (zufällige Nonce vorangestellt).
pub fn seal(key: &[u8; 32], msg: &[u8], aad: &[u8]) -> Vec<u8> {
    let nonce = random_bytes::<24>();
    let ct = cipher(key)
        .encrypt(XNonce::from_slice(&nonce), Payload { msg, aad })
        .expect("XChaCha20-Poly1305 Verschlüsselung schlägt nicht fehl");
    let mut out = Vec::with_capacity(24 + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    out
}

pub fn open(key: &[u8; 32], blob: &[u8], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if blob.len() < 24 + TAG_LEN {
        return Err(Error::WrongPassword);
    }
    let (nonce, ct) = blob.split_at(24);
    cipher(key)
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad })
        .map(Zeroizing::new)
        .map_err(|_| Error::WrongPassword)
}

// ---------------------------------------------------------------------------
// Wiederherstellungsschlüssel: 32 Byte als Base32 in 4er-Gruppen.

pub fn format_recovery(raw: &[u8; 32]) -> String {
    let s = data_encoding::BASE32_NOPAD.encode(raw);
    s.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn parse_recovery(s: &str) -> Result<Zeroizing<[u8; 32]>> {
    let cleaned: String = s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let bytes = data_encoding::BASE32_NOPAD
        .decode(cleaned.as_bytes())
        .map_err(|_| Error::InvalidInput("Wiederherstellungsschlüssel ist ungültig".into()))?;
    if bytes.len() != 32 {
        return Err(Error::InvalidInput("Wiederherstellungsschlüssel ist ungültig".into()));
    }
    let mut out = Zeroizing::new([0u8; 32]);
    out.copy_from_slice(&bytes);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Blockweise Objektverschlüsselung

#[derive(Clone)]
struct ObjectHeader {
    chunk_size: u32,
    nonce_prefix: [u8; 16],
    plain_len: u64,
    raw: [u8; HEADER_LEN],
}

impl ObjectHeader {
    fn new(plain_len: u64) -> Self {
        let nonce_prefix = random_bytes::<16>();
        let mut raw = [0u8; HEADER_LEN];
        raw[0..4].copy_from_slice(MAGIC);
        raw[4] = FORMAT_VERSION;
        raw[5..9].copy_from_slice(&CHUNK_SIZE.to_le_bytes());
        raw[9..25].copy_from_slice(&nonce_prefix);
        raw[25..33].copy_from_slice(&plain_len.to_le_bytes());
        Self { chunk_size: CHUNK_SIZE, nonce_prefix, plain_len, raw }
    }

    fn parse(raw: [u8; HEADER_LEN]) -> Result<Self> {
        if &raw[0..4] != MAGIC || raw[4] != FORMAT_VERSION {
            return Err(Error::Corrupt("unbekanntes Objektformat".into()));
        }
        let chunk_size = u32::from_le_bytes(raw[5..9].try_into().unwrap());
        if chunk_size == 0 || chunk_size > 64 * 1024 * 1024 {
            return Err(Error::Corrupt("ungültige Blockgröße".into()));
        }
        let mut nonce_prefix = [0u8; 16];
        nonce_prefix.copy_from_slice(&raw[9..25]);
        let plain_len = u64::from_le_bytes(raw[25..33].try_into().unwrap());
        Ok(Self { chunk_size, nonce_prefix, plain_len, raw })
    }

    fn chunk_count(&self) -> u64 {
        let cs = self.chunk_size as u64;
        if self.plain_len == 0 {
            1
        } else {
            self.plain_len.div_ceil(cs)
        }
    }

    fn chunk_plain_len(&self, idx: u64) -> usize {
        let cs = self.chunk_size as u64;
        let start = idx * cs;
        (self.plain_len.saturating_sub(start)).min(cs) as usize
    }

    fn chunk_offset(&self, idx: u64) -> u64 {
        HEADER_LEN as u64 + idx * (self.chunk_size as u64 + TAG_LEN as u64)
    }

    fn nonce(&self, idx: u64) -> XNonce {
        let mut n = [0u8; 24];
        n[..16].copy_from_slice(&self.nonce_prefix);
        n[16..].copy_from_slice(&idx.to_le_bytes());
        *XNonce::from_slice(&n)
    }

    fn aad(&self, object_id: &str, idx: u64) -> Vec<u8> {
        let last = idx + 1 == self.chunk_count();
        let mut aad = Vec::with_capacity(HEADER_LEN + object_id.len() + 9);
        aad.extend_from_slice(&self.raw);
        aad.extend_from_slice(object_id.as_bytes());
        aad.extend_from_slice(&idx.to_le_bytes());
        aad.push(last as u8);
        aad
    }
}

/// Verschlüsselt genau `plain_len` Bytes aus `reader` nach `writer`.
/// Liefert den SHA-256 des Klartexts.
pub fn encrypt_stream<R: Read, W: Write>(
    key: &[u8; 32],
    object_id: &str,
    plain_len: u64,
    reader: &mut R,
    writer: &mut W,
) -> Result<[u8; 32]> {
    let header = ObjectHeader::new(plain_len);
    let c = cipher(key);
    let mut hasher = Sha256::new();
    writer.write_all(&header.raw)?;
    let mut buf = Zeroizing::new(vec![0u8; header.chunk_size as usize]);
    for idx in 0..header.chunk_count() {
        let n = header.chunk_plain_len(idx);
        reader.read_exact(&mut buf[..n]).map_err(|e| {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                Error::Io("Datei wurde während des Lesens verändert".into())
            } else {
                e.into()
            }
        })?;
        hasher.update(&buf[..n]);
        let aad = header.aad(object_id, idx);
        let ct = c
            .encrypt(&header.nonce(idx), Payload { msg: &buf[..n], aad: &aad })
            .expect("Verschlüsselung schlägt nicht fehl");
        writer.write_all(&ct)?;
    }
    Ok(hasher.finalize().into())
}

pub fn encrypt_bytes(key: &[u8; 32], object_id: &str, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + HEADER_LEN + 64);
    let mut r = data;
    encrypt_stream(key, object_id, data.len() as u64, &mut r, &mut out)
        .expect("Verschlüsselung im Speicher schlägt nicht fehl");
    out
}

pub fn decrypt_bytes(key: &[u8; 32], object_id: &str, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let mut reader = ObjectReader::new(io::Cursor::new(blob), key, object_id)?;
    let mut out = Zeroizing::new(Vec::with_capacity(reader.len() as usize));
    reader.read_to_end(&mut out)?;
    Ok(out)
}

/// Entschlüsselnder Leser mit wahlfreiem Zugriff (für Video-Spulen und Export).
pub struct ObjectReader<F: Read + Seek> {
    file: F,
    cipher: XChaCha20Poly1305,
    object_id: String,
    header: ObjectHeader,
    pos: u64,
    chunk: Zeroizing<Vec<u8>>,
    chunk_idx: Option<u64>,
}

impl<F: Read + Seek> ObjectReader<F> {
    pub fn new(mut file: F, key: &[u8; 32], object_id: &str) -> Result<Self> {
        file.seek(SeekFrom::Start(0))?;
        let mut raw = [0u8; HEADER_LEN];
        file.read_exact(&mut raw)
            .map_err(|_| Error::Corrupt("Objekt zu kurz".into()))?;
        let header = ObjectHeader::parse(raw)?;
        Ok(Self {
            file,
            cipher: cipher(key),
            object_id: object_id.to_string(),
            header,
            pos: 0,
            chunk: Zeroizing::new(Vec::new()),
            chunk_idx: None,
        })
    }

    pub fn len(&self) -> u64 {
        self.header.plain_len
    }

    fn load_chunk(&mut self, idx: u64) -> io::Result<()> {
        if self.chunk_idx == Some(idx) {
            return Ok(());
        }
        let n = self.header.chunk_plain_len(idx) + TAG_LEN;
        let mut ct = vec![0u8; n];
        self.file.seek(SeekFrom::Start(self.header.chunk_offset(idx)))?;
        self.file.read_exact(&mut ct).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "verschlüsseltes Objekt ist unvollständig")
        })?;
        let aad = self.header.aad(&self.object_id, idx);
        let plain = self
            .cipher
            .decrypt(&self.header.nonce(idx), Payload { msg: &ct, aad: &aad })
            .map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Integritätsprüfung fehlgeschlagen")
            })?;
        self.chunk = Zeroizing::new(plain);
        self.chunk_idx = Some(idx);
        Ok(())
    }

    /// Liest den Bereich `[start, end)` des Klartexts.
    pub fn read_range(&mut self, start: u64, end: u64) -> io::Result<Zeroizing<Vec<u8>>> {
        let end = end.min(self.len());
        let mut out = Zeroizing::new(Vec::with_capacity(end.saturating_sub(start) as usize));
        self.seek(SeekFrom::Start(start))?;
        let mut remaining = end.saturating_sub(start);
        let mut buf = Zeroizing::new(vec![0u8; 256 * 1024]);
        while remaining > 0 {
            let want = remaining.min(buf.len() as u64) as usize;
            let n = self.read(&mut buf[..want])?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
            remaining -= n as u64;
        }
        Ok(out)
    }
}

impl<F: Read + Seek> Read for ObjectReader<F> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len() || out.is_empty() {
            // Leere Objekte trotzdem authentifizieren.
            if self.len() == 0 && self.chunk_idx.is_none() {
                self.load_chunk(0)?;
            }
            return Ok(0);
        }
        let cs = self.header.chunk_size as u64;
        let idx = self.pos / cs;
        self.load_chunk(idx)?;
        let off = (self.pos - idx * cs) as usize;
        let n = (self.chunk.len() - off).min(out.len());
        out[..n].copy_from_slice(&self.chunk[off..off + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl<F: Read + Seek> Seek for ObjectReader<F> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(d) => self.len() as i128 + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if new < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Position vor Dateianfang"));
        }
        self.pos = new as u64;
        Ok(self.pos)
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; 32] {
        [7u8; 32]
    }

    #[test]
    fn roundtrip_various_sizes() {
        let cs = CHUNK_SIZE as usize;
        for len in [0, 1, 1000, cs - 1, cs, cs + 1, 3 * cs + 17] {
            let data: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
            let blob = encrypt_bytes(&key(), "o:test", &data);
            let back = decrypt_bytes(&key(), "o:test", &blob).unwrap();
            assert_eq!(&back[..], &data[..], "Länge {len}");
        }
    }

    #[test]
    fn wrong_key_or_id_fails() {
        let blob = encrypt_bytes(&key(), "o:a", b"geheim");
        assert!(decrypt_bytes(&[8u8; 32], "o:a", &blob).is_err());
        assert!(decrypt_bytes(&key(), "o:b", &blob).is_err());
    }

    #[test]
    fn tampering_is_detected() {
        let data = vec![42u8; CHUNK_SIZE as usize * 2 + 5];
        let mut blob = encrypt_bytes(&key(), "o:x", &data);
        let mid = blob.len() / 2;
        blob[mid] ^= 1;
        assert!(decrypt_bytes(&key(), "o:x", &blob).is_err());
    }

    #[test]
    fn truncation_is_detected() {
        let data = vec![1u8; CHUNK_SIZE as usize * 2 + 5];
        let blob = encrypt_bytes(&key(), "o:x", &data);
        // letzten Block entfernen
        let cut = HEADER_LEN + 2 * (CHUNK_SIZE as usize + TAG_LEN);
        assert!(decrypt_bytes(&key(), "o:x", &blob[..cut]).is_err());
        // Länge im Header manipulieren
        let mut b2 = blob.clone();
        b2[25] = b2[25].wrapping_sub(5);
        assert!(decrypt_bytes(&key(), "o:x", &b2).is_err());
    }

    #[test]
    fn random_access_ranges() {
        let cs = CHUNK_SIZE as u64;
        let data: Vec<u8> = (0..(3 * cs + 100)).map(|i| (i % 253) as u8).collect();
        let blob = encrypt_bytes(&key(), "o:r", &data);
        let mut r = ObjectReader::new(io::Cursor::new(blob), &key(), "o:r").unwrap();
        for (s, e) in [(0, 10), (cs - 5, cs + 5), (2 * cs + 1, 3 * cs + 100), (10, 3 * cs + 99)] {
            let got = r.read_range(s, e).unwrap();
            assert_eq!(&got[..], &data[s as usize..e as usize]);
        }
    }

    #[test]
    fn key_wrapping() {
        let kek = derive_password_key("passwort", b"saltsaltsaltsalt", &KdfParams::fast_for_tests()).unwrap();
        let master = random_key();
        let wrapped = wrap_key(&kek, &master, b"aad");
        assert_eq!(*unwrap_key(&kek, &wrapped, b"aad").unwrap(), *master);
        let wrong = derive_password_key("Passwort", b"saltsaltsaltsalt", &KdfParams::fast_for_tests()).unwrap();
        assert!(matches!(unwrap_key(&wrong, &wrapped, b"aad"), Err(Error::WrongPassword)));
    }

    #[test]
    fn recovery_format_roundtrip() {
        let raw = random_bytes::<32>();
        let s = format_recovery(&raw);
        assert_eq!(*parse_recovery(&s).unwrap(), raw);
        assert_eq!(*parse_recovery(&s.to_lowercase().replace('-', " ")).unwrap(), raw);
        assert!(parse_recovery("ABCD-EFGH").is_err());
    }
}
