//! Worker-issued receipts. The authentication key must never enter build containers.
use crate::{storage::Error, types::BuildTaskTransmissionFormat};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Receipt {
    pub sha256: String,
    pub bytes: u64,
    pub mac: String,
}
pub fn digest(path: &Path) -> Result<(String, u64), Error> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        bytes += n as u64;
    }
    Ok((hex::encode(hash.finalize()), bytes))
}
fn authentication(
    key: &str,
    task: &BuildTaskTransmissionFormat,
    sha256: &str,
    bytes: u64,
) -> Result<Hmac<Sha256>, Error> {
    if key.len() < 32 {
        return Err("AB_ARTIFACT_KEY must contain at least 32 bytes".into());
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())?;
    mac.update(&serde_json::to_vec(&(
        "aur-builder-artifact-v1",
        task.id,
        &task.candidate_id,
        &task.revision,
        &task.attempt,
        &task.object_key,
        sha256,
        bytes,
    ))?);
    Ok(mac)
}
pub fn issue(key: &str, task: &BuildTaskTransmissionFormat, path: &Path) -> Result<Receipt, Error> {
    let (sha256, bytes) = digest(path)?;
    let mac = hex::encode(
        authentication(key, task, &sha256, bytes)?
            .finalize()
            .into_bytes(),
    );
    Ok(Receipt { sha256, bytes, mac })
}
pub fn verify(
    key: &str,
    task: &BuildTaskTransmissionFormat,
    path: &Path,
    receipt: &Receipt,
) -> Result<(), Error> {
    if receipt.sha256.len() != 64
        || receipt.mac.len() != 64
        || !receipt.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || !receipt.mac.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("invalid artifact receipt encoding".into());
    }
    authentication(key, task, &receipt.sha256, receipt.bytes)?
        .verify_slice(&hex::decode(&receipt.mac)?)?;
    let (hash, bytes) = digest(path)?;
    if hash != receipt.sha256 || bytes != receipt.bytes {
        return Err("artifact does not match worker receipt".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_binds_bytes_attempt_and_key() {
        let task: BuildTaskTransmissionFormat = serde_json::from_value(serde_json::json!({"id":1,"candidate_id":"candidate","revision":"a".repeat(40),"attempt":"attempt","object_key":"builds/attempt/bundle.tar","name":"test","version":"1","source":null,"subfolder":null,"options":null,"env":null})).unwrap();
        let path = std::env::temp_dir().join(format!("artifact-test-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"original artifact").unwrap();
        let key = "outside-sandbox-secret-key-32-bytes";
        let receipt = issue(key, &task, &path).unwrap();
        verify(key, &task, &path, &receipt).unwrap();
        assert!(
            verify(
                "different-outside-sandbox-secret-key",
                &task,
                &path,
                &receipt
            )
            .is_err()
        );
        let mut other = task.clone();
        other.attempt = "another-attempt".into();
        assert!(verify(key, &other, &path, &receipt).is_err());
        std::fs::write(&path, b"replacement artifact").unwrap();
        assert!(verify(key, &task, &path, &receipt).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
