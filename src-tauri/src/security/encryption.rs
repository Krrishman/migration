//! Optional encryption at rest for bundle payload files.
//!
//! * Key derivation: Argon2id (64 MiB, 3 passes, 1 lane) from the passphrase
//!   and a random 16-byte bundle salt.
//! * Per-file key: HKDF-SHA256(master key, random 32-byte file salt).
//! * Payload: AES-256-GCM in the STREAM construction (big-endian 32-bit
//!   counter, last-block flag) over 1 MiB chunks, with a random 7-byte nonce
//!   prefix per file. Truncation, reordering and tampering are detected.
//! * The key and passphrase are never written to disk or logs. The manifest
//!   stores only KDF parameters and a key-check value.
//!
//! File layout: `MAGIC(8) | file_salt(32) | nonce_prefix(7) | chunk...`
//! where each ciphertext chunk is `CHUNK_SIZE + 16` bytes except the last.

use crate::error::{AppError, AppResult};
use crate::models::{EncryptionMetadata, KdfParams};
use aes_gcm::aead::stream::{DecryptorBE32, EncryptorBE32};
use aes_gcm::aead::{generic_array::GenericArray, Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use std::io::{Read, Write};
use zeroize::Zeroizing;

pub const ALGORITHM: &str = "aes-256-gcm-stream-be32";
pub const KDF_ALGORITHM: &str = "argon2id-v19";
pub const CHUNK_SIZE: usize = 1024 * 1024;
pub const MAGIC: &[u8; 8] = b"MAENC01\0";
pub const ENCRYPTED_EXTENSION: &str = "maenc";
pub const MIN_PASSPHRASE_CHARS: usize = 12;
const TAG_SIZE: usize = 16;
const FILE_SALT_LEN: usize = 32;
const NONCE_PREFIX_LEN: usize = 7;
const KEY_CHECK_PLAINTEXT: &[u8] = b"migration-assistant key check v1";
const HKDF_INFO: &[u8] = b"migration-assistant file key v1";

/// Derived 256-bit bundle key. Zeroed on drop; deliberately not `Debug`/`Serialize`.
pub struct BundleKey(Zeroizing<[u8; 32]>);

impl BundleKey {
    fn file_cipher(&self, file_salt: &[u8]) -> AppResult<Aes256Gcm> {
        let hk = Hkdf::<Sha256>::new(Some(file_salt), self.0.as_ref());
        let mut okm = Zeroizing::new([0u8; 32]);
        hk.expand(HKDF_INFO, okm.as_mut()).map_err(|_| AppError::Crypto("key expansion failed".into()))?;
        Ok(Aes256Gcm::new(GenericArray::from_slice(okm.as_ref())))
    }
}

/// Validate the passphrase and its confirmation. Error messages never echo input.
pub fn validate_passphrase(passphrase: &str, confirmation: &str) -> AppResult<()> {
    if passphrase != confirmation {
        return Err(AppError::Crypto("passphrase and confirmation do not match".into()));
    }
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(AppError::Crypto(format!("passphrase must be at least {MIN_PASSPHRASE_CHARS} characters")));
    }
    Ok(())
}

/// New KDF parameters with a fresh random salt.
pub fn new_kdf_params() -> KdfParams {
    let mut salt = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    KdfParams { algorithm: KDF_ALGORITHM.into(), salt: hex::encode(salt), memory_kib: 64 * 1024, iterations: 3, parallelism: 1 }
}

pub fn derive_key(passphrase: &str, params: &KdfParams) -> AppResult<BundleKey> {
    if params.algorithm != KDF_ALGORITHM {
        return Err(AppError::Crypto(format!("unsupported KDF {}", params.algorithm)));
    }
    let salt = hex::decode(&params.salt).map_err(|_| AppError::Crypto("invalid KDF salt".into()))?;
    if salt.len() < 16 {
        return Err(AppError::Crypto("KDF salt too short".into()));
    }
    // Bound parameters so a hostile manifest cannot request absurd memory.
    if params.memory_kib > 1024 * 1024 || params.iterations > 20 || params.parallelism > 16 || params.memory_kib < 8 {
        return Err(AppError::Crypto("KDF parameters out of accepted range".into()));
    }
    let argon = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(params.memory_kib, params.iterations, params.parallelism, Some(32))
            .map_err(|e| AppError::Crypto(format!("invalid KDF parameters: {e}")))?,
    );
    let mut out = Zeroizing::new([0u8; 32]);
    argon.hash_password_into(passphrase.as_bytes(), &salt, out.as_mut()).map_err(|e| AppError::Crypto(format!("key derivation failed: {e}")))?;
    Ok(BundleKey(out))
}

/// Encrypt a fixed constant so a later passphrase can be checked before any
/// payload is touched. Returns hex(nonce || ciphertext).
pub fn make_key_check(key: &BundleKey) -> AppResult<String> {
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key.0.as_ref()));
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let ct = cipher.encrypt(Nonce::from_slice(&nonce), KEY_CHECK_PLAINTEXT).map_err(|_| AppError::Crypto("key check encryption failed".into()))?;
    let mut out = nonce.to_vec();
    out.extend(ct);
    Ok(hex::encode(out))
}

pub fn verify_key_check(key: &BundleKey, check_hex: &str) -> bool {
    let Ok(raw) = hex::decode(check_hex) else { return false };
    if raw.len() < 12 + TAG_SIZE {
        return false;
    }
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key.0.as_ref()));
    matches!(cipher.decrypt(Nonce::from_slice(&raw[..12]), &raw[12..]), Ok(pt) if pt == KEY_CHECK_PLAINTEXT)
}

/// Build manifest metadata for a new encrypted bundle and return the key.
pub fn setup_bundle_encryption(passphrase: &str) -> AppResult<(EncryptionMetadata, BundleKey)> {
    let kdf = new_kdf_params();
    let key = derive_key(passphrase, &kdf)?;
    let check = make_key_check(&key)?;
    Ok((
        EncryptionMetadata { enabled: true, algorithm: Some(ALGORITHM.into()), kdf: Some(kdf), key_check: Some(check), chunk_size: Some(CHUNK_SIZE as u32) },
        key,
    ))
}

/// Unlock an existing bundle. Fails with a generic message on a wrong passphrase.
pub fn unlock_bundle(meta: &EncryptionMetadata, passphrase: &str) -> AppResult<BundleKey> {
    if !meta.enabled {
        return Err(AppError::Crypto("bundle is not encrypted".into()));
    }
    if meta.algorithm.as_deref() != Some(ALGORITHM) {
        return Err(AppError::Crypto("unsupported encryption algorithm".into()));
    }
    let kdf = meta.kdf.as_ref().ok_or_else(|| AppError::Crypto("missing KDF parameters".into()))?;
    let check = meta.key_check.as_deref().ok_or_else(|| AppError::Crypto("missing key check".into()))?;
    let key = derive_key(passphrase, kdf)?;
    if !verify_key_check(&key, check) {
        return Err(AppError::Crypto("incorrect passphrase".into()));
    }
    Ok(key)
}

fn read_full<R: Read>(r: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

fn io(e: std::io::Error) -> AppError {
    AppError::Io { path: "<stream>".into(), source: e }
}

/// Encrypt `reader` into `writer`. `on_plain` sees each plaintext chunk (used
/// to hash the original content in the same pass). Returns bytes written.
pub fn encrypt_stream<R: Read, W: Write>(key: &BundleKey, mut reader: R, mut writer: W, mut on_plain: impl FnMut(&[u8])) -> AppResult<u64> {
    let mut file_salt = [0u8; FILE_SALT_LEN];
    let mut nonce = [0u8; NONCE_PREFIX_LEN];
    rand::rngs::OsRng.fill_bytes(&mut file_salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let cipher = key.file_cipher(&file_salt)?;
    let mut enc = EncryptorBE32::from_aead(cipher, GenericArray::from_slice(&nonce));
    writer.write_all(MAGIC).map_err(io)?;
    writer.write_all(&file_salt).map_err(io)?;
    writer.write_all(&nonce).map_err(io)?;
    let mut written = (MAGIC.len() + FILE_SALT_LEN + NONCE_PREFIX_LEN) as u64;

    let mut cur = vec![0u8; CHUNK_SIZE];
    let mut next = vec![0u8; CHUNK_SIZE];
    let mut cur_len = read_full(&mut reader, &mut cur).map_err(io)?;
    loop {
        let next_len = if cur_len == CHUNK_SIZE { read_full(&mut reader, &mut next).map_err(io)? } else { 0 };
        on_plain(&cur[..cur_len]);
        if next_len == 0 {
            let ct = enc.encrypt_last(&cur[..cur_len]).map_err(|_| AppError::Crypto("encryption failed".into()))?;
            writer.write_all(&ct).map_err(io)?;
            written += ct.len() as u64;
            break;
        }
        let ct = enc.encrypt_next(&cur[..cur_len]).map_err(|_| AppError::Crypto("encryption failed".into()))?;
        writer.write_all(&ct).map_err(io)?;
        written += ct.len() as u64;
        std::mem::swap(&mut cur, &mut next);
        cur_len = next_len;
    }
    writer.flush().map_err(io)?;
    Ok(written)
}

/// Decrypt a stream produced by [`encrypt_stream`]. Any tampering, truncation
/// or wrong key yields an error; partial plaintext must then be discarded by
/// the caller (restore writes to a temp file and only renames on success).
pub fn decrypt_stream<R: Read, W: Write>(key: &BundleKey, mut reader: R, mut writer: W, mut on_plain: impl FnMut(&[u8])) -> AppResult<u64> {
    let mut header = [0u8; 8 + FILE_SALT_LEN + NONCE_PREFIX_LEN];
    if read_full(&mut reader, &mut header).map_err(io)? != header.len() || &header[..8] != MAGIC {
        return Err(AppError::Crypto("not a Migration Assistant encrypted file".into()));
    }
    let file_salt = &header[8..8 + FILE_SALT_LEN];
    let nonce = &header[8 + FILE_SALT_LEN..];
    let cipher = key.file_cipher(file_salt)?;
    let mut dec = Some(DecryptorBE32::from_aead(cipher, GenericArray::from_slice(nonce)));
    let block = CHUNK_SIZE + TAG_SIZE;
    let mut cur = vec![0u8; block];
    let mut next = vec![0u8; block];
    let mut cur_len = read_full(&mut reader, &mut cur).map_err(io)?;
    let mut total = 0u64;
    loop {
        let next_len = if cur_len == block { read_full(&mut reader, &mut next).map_err(io)? } else { 0 };
        let pt = if next_len == 0 {
            dec.take()
                .expect("decryptor consumed only once")
                .decrypt_last(&cur[..cur_len])
                .map_err(|_| AppError::Crypto("authentication failed (wrong key, corrupted or truncated file)".into()))?
        } else {
            dec.as_mut()
                .expect("decryptor present until last chunk")
                .decrypt_next(&cur[..cur_len])
                .map_err(|_| AppError::Crypto("authentication failed (wrong key or corrupted file)".into()))?
        };
        on_plain(&pt);
        writer.write_all(&pt).map_err(io)?;
        total += pt.len() as u64;
        if next_len == 0 {
            break;
        }
        std::mem::swap(&mut cur, &mut next);
        cur_len = next_len;
    }
    writer.flush().map_err(io)?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_key(pass: &str) -> (EncryptionMetadata, BundleKey) {
        // Use reduced KDF cost in tests only.
        let mut kdf = new_kdf_params();
        kdf.memory_kib = 1024;
        kdf.iterations = 1;
        let key = derive_key(pass, &kdf).unwrap();
        let check = make_key_check(&key).unwrap();
        (
            EncryptionMetadata {
                enabled: true,
                algorithm: Some(ALGORITHM.into()),
                kdf: Some(kdf),
                key_check: Some(check),
                chunk_size: Some(CHUNK_SIZE as u32),
            },
            key,
        )
    }

    #[test]
    fn passphrase_validation() {
        assert!(validate_passphrase("short", "short").is_err());
        assert!(validate_passphrase("correct horse battery", "correct horse batterY").is_err());
        assert!(validate_passphrase("correct horse battery", "correct horse battery").is_ok());
    }

    #[test]
    fn metadata_contains_no_key_material() {
        let (meta, _key) = fast_key("correct horse battery");
        let json = serde_json::to_string(&meta).unwrap();
        assert!(!json.contains("correct horse"));
        assert!(json.contains(ALGORITHM));
        assert!(json.contains("argon2id"));
        assert_eq!(meta.kdf.unwrap().salt.len(), 32);
    }

    #[test]
    fn unlock_rejects_wrong_passphrase() {
        let (meta, _) = fast_key("correct horse battery");
        assert!(unlock_bundle(&meta, "correct horse battery").is_ok());
        assert!(unlock_bundle(&meta, "wrong horse battery!").is_err());
    }

    #[test]
    fn roundtrip_various_sizes() {
        let (_, key) = fast_key("correct horse battery");
        for size in [0usize, 1, 1000, CHUNK_SIZE - 1, CHUNK_SIZE, CHUNK_SIZE + 1, 2 * CHUNK_SIZE + 5] {
            let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            let mut enc = Vec::new();
            encrypt_stream(&key, &data[..], &mut enc, |_| {}).unwrap();
            assert!(enc.len() >= 8 + 32 + 7 + 16 + data.len());
            if size >= 64 {
                assert!(!enc.windows(64).any(|w| w == &data[..64]), "plaintext must not appear in ciphertext");
            }
            let mut dec = Vec::new();
            decrypt_stream(&key, &enc[..], &mut dec, |_| {}).unwrap();
            assert_eq!(dec, data, "size {size}");
        }
    }

    #[test]
    fn nonces_differ_per_file() {
        let (_, key) = fast_key("correct horse battery");
        let mut a = Vec::new();
        let mut b = Vec::new();
        encrypt_stream(&key, &b"same"[..], &mut a, |_| {}).unwrap();
        encrypt_stream(&key, &b"same"[..], &mut b, |_| {}).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn tamper_and_truncation_detected() {
        let (_, key) = fast_key("correct horse battery");
        let data = vec![7u8; CHUNK_SIZE + 100];
        let mut enc = Vec::new();
        encrypt_stream(&key, &data[..], &mut enc, |_| {}).unwrap();
        let mut tampered = enc.clone();
        let mid = tampered.len() / 2;
        tampered[mid] ^= 1;
        assert!(decrypt_stream(&key, &tampered[..], &mut Vec::new(), |_| {}).is_err());
        // Drop the final chunk entirely: the remaining full chunk is not marked last.
        let truncated = &enc[..8 + 32 + 7 + CHUNK_SIZE + 16];
        assert!(decrypt_stream(&key, truncated, &mut Vec::new(), |_| {}).is_err());
        let (_, other) = fast_key("another passphrase!");
        assert!(decrypt_stream(&other, &enc[..], &mut Vec::new(), |_| {}).is_err());
    }

    #[test]
    fn hostile_kdf_parameters_rejected() {
        let mut kdf = new_kdf_params();
        kdf.memory_kib = u32::MAX;
        assert!(derive_key("correct horse battery", &kdf).is_err());
    }
}
