use crate::{MetaError, Result};
use sha2::{Digest, Sha256, Sha512};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Algorithm {
    Sha256,
    Sha512,
}

impl Algorithm {
    pub fn name(self) -> &'static str {
        match self { Algorithm::Sha256 => "sha256", Algorithm::Sha512 => "sha512" }
    }
}

/// Streaming digest of a file with the chosen algorithm.
pub fn digest_file(path: &Path, alg: Algorithm) -> Result<String> {
    match alg {
        Algorithm::Sha256 => sha256_file(path),
        Algorithm::Sha512 => {
            let mut f = std::fs::File::open(path).map_err(|e| MetaError::io(path, e))?;
            let mut h = Sha512::new();
            let mut buf = vec![0u8; 1 << 16];
            loop {
                let n = f.read(&mut buf).map_err(|e| MetaError::io(path, e))?;
                if n == 0 { break; }
                h.update(&buf[..n]);
            }
            Ok(hex::encode(h.finalize()))
        }
    }
}

pub fn sha256_bytes(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// Streaming SHA-256 (never loads the whole file into memory).
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path).map_err(|e| MetaError::io(path, e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|e| MetaError::io(path, e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_vector() {
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
    #[test]
    fn file_matches_bytes() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), sha256_bytes(b"abc"));
    }
}
