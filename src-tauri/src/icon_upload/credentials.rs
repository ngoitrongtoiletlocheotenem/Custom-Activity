//! Per-user DPAPI credential, additionally bound to the configured endpoint.
//! The on-disk blob is never returned to the renderer.
use std::path::{Path, PathBuf};

pub struct TokenStore {
    path: PathBuf,
}
impl TokenStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn credential_stored(&self, endpoint: &str) -> bool {
        self.load(endpoint).is_ok()
    }
    pub fn snapshot(&self) -> Option<Vec<u8>> {
        std::fs::read(&self.path).ok()
    }
    pub fn restore(&self, snapshot: Option<Vec<u8>>) {
        if let Some(blob) = snapshot {
            let _ = write_private_blob(&self.path, &blob);
        } else {
            let _ = std::fs::remove_file(&self.path);
        }
    }
    pub fn save(&self, endpoint: &str, token: &str) -> Result<(), String> {
        validate_token(token)?;
        let encrypted = protect(token.as_bytes(), endpoint.as_bytes(), true)?;
        write_private_blob(&self.path, &encrypted)
    }
    pub fn load(&self, endpoint: &str) -> Result<String, String> {
        let blob =
            std::fs::read(&self.path).map_err(|_| "Icon upload credential is unavailable.")?;
        if blob.len() > 32 * 1024 {
            return Err("Icon upload credential is invalid.".into());
        }
        let bytes = protect(&blob, endpoint.as_bytes(), false)?;
        let token = String::from_utf8(bytes).map_err(|_| "Icon upload credential is invalid.")?;
        validate_token(&token)?;
        Ok(token)
    }
}
pub fn validate_token(token: &str) -> Result<(), String> {
    if token.len() < 32 || token.len() > 1024 || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(
            "Worker token must contain 32–1024 printable non-space ASCII characters.".into(),
        );
    }
    Ok(())
}
fn write_private_blob(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or("Credential directory is unavailable.")?;
    std::fs::create_dir_all(parent).map_err(|_| "Cannot create credential directory.")?;
    let temporary = path.with_extension("tmp");
    let mut file =
        std::fs::File::create(&temporary).map_err(|_| "Cannot save encrypted credential.")?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Cannot save encrypted credential.")?;
    drop(file);
    std::fs::rename(temporary, path).map_err(|_| "Cannot replace encrypted credential.".into())
}
#[cfg(windows)]
fn protect(bytes: &[u8], entropy: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::*};
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr().cast_mut(),
        };
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: entropy.len() as u32,
            pbData: entropy.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(
                "Windows could not access the per-user encrypted icon upload credential.".into(),
            );
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}
#[cfg(not(windows))]
fn protect(_: &[u8], _: &[u8], _: bool) -> Result<Vec<u8>, String> {
    Err("Icon upload credentials require Windows DPAPI.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authorization_header_injection_rejected() {
        assert!(validate_token("abcdefghijklmnop1234567890123456\r\nx:y").is_err());
        assert!(validate_token("abcdefghijklmnop1234567890123456").is_ok());
    }
    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip_is_bound_to_endpoint_and_not_plaintext() {
        let secret = b"test-only-token-123456789";
        let blob = protect(secret, b"https://worker.example.com", true).unwrap();
        assert!(!blob.windows(secret.len()).any(|window| window == secret));
        assert_eq!(
            protect(&blob, b"https://worker.example.com", false).unwrap(),
            secret
        );
        assert!(protect(&blob, b"https://other.example.com", false).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn encrypted_file_can_be_replaced_without_exposing_token() {
        let directory = std::env::temp_dir().join(format!(
            "ca-dpapi-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = TokenStore::new(directory.join("token.dpapi"));
        let endpoint = "https://upload.example.com";
        let first = "test-only-first-token-12345678901234567890";
        let second = "test-only-second-token-12345678901234567890";
        store.save(endpoint, first).unwrap();
        store.save(endpoint, second).unwrap();
        assert_eq!(store.load(endpoint).unwrap(), second);
        assert!(!std::fs::read(directory.join("token.dpapi"))
            .unwrap()
            .windows(second.len())
            .any(|window| window == second.as_bytes()));
        std::fs::remove_file(directory.join("token.dpapi")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
