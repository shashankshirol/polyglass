use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 of a file, streamed in 64 KiB chunks.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// True if the file's SHA-256 equals `expected` (lowercase hex).
pub fn verify_file(path: &Path, expected: &str) -> io::Result<bool> {
    Ok(sha256_file(path)? == expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_digest() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t");
        std::fs::write(&p, b"test").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), crate::manifest::tests::HASH);
        assert!(verify_file(&p, crate::manifest::tests::HASH).unwrap());
    }
}
