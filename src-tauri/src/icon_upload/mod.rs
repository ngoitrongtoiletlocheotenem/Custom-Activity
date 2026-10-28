//! Optional, explicitly consented upload of selected executable PNG bytes only.
pub mod credentials;
mod worker;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
pub use worker::UploadCoordinator;

pub const MAX_PNG_BYTES: usize = 128 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct IconUploadSettings {
    pub enabled: bool,
    pub consent_granted: bool,
    pub endpoint: String,
    pub public_base: String,
    /// Derived from the DPAPI store, never accepted from renderer input.
    #[serde(skip_deserializing)]
    pub credential_stored: bool,
}
impl Default for IconUploadSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            consent_granted: false,
            endpoint: String::new(),
            public_base: option_env!("CURRENTACTIVITY_ICON_PUBLIC_BASE")
                .unwrap_or("")
                .into(),
            credential_stored: false,
        }
    }
}
impl IconUploadSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.enabled && !self.consent_granted {
            return Err("Explicit icon upload consent is required.".into());
        }
        if self.enabled || !self.endpoint.is_empty() {
            validate_base(&self.endpoint)?;
        }
        if !self.public_base.is_empty() {
            validate_base(&self.public_base)?;
        }
        Ok(())
    }
    pub fn normalized(mut self) -> Self {
        self.endpoint = self.endpoint.trim_end_matches('/').into();
        self.public_base = self.public_base.trim_end_matches('/').into();
        self
    }
    pub fn fingerprint(&self) -> String {
        hash(format!("{}\n{}", self.endpoint, self.public_base).as_bytes())
    }
}
pub fn validate_base(value: &str) -> Result<(), String> {
    let url =
        Url::parse(value).map_err(|_| "Icon hosting address must be a public HTTPS origin.")?;
    if !value.starts_with("https://")
        || !crate::discord::provider::is_valid_discord_image(value)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
        || url.port().is_some_and(|port| port != 443)
    {
        return Err("Icon hosting address must be a public HTTPS origin without credentials, path, query or fragment.".into());
    }
    Ok(())
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn png_bytes(data_url: &str) -> Result<Vec<u8>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let encoded = data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or("Selected application has no PNG icon.")?;
    if encoded.len() > MAX_PNG_BYTES.div_ceil(3) * 4 {
        return Err("Executable icon is too large to upload.".into());
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "Executable icon PNG is invalid.")?;
    if bytes.len() > MAX_PNG_BYTES
        || bytes.len() < 24
        || bytes[..8] != *b"\x89PNG\r\n\x1a\n"
        || &bytes[12..16] != b"IHDR"
    {
        return Err("Executable icon PNG is invalid or too large.".into());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0 || height == 0 || width > 256 || height > 256 {
        return Err("Executable icon dimensions are unsupported.".into());
    }
    // Decode as PNG too: header bytes alone do not establish valid artwork.
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|_| "Executable icon PNG is invalid.")?;
    Ok(bytes)
}
pub fn validate_response(
    settings: &IconUploadSettings,
    content_hash: &str,
    url: &str,
    returned_hash: &str,
) -> Result<(), String> {
    if returned_hash != content_hash
        || content_hash.len() != 64
        || !content_hash.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Icon host returned a mismatched content identity.".into());
    }
    if !crate::discord::provider::is_valid_discord_image(url) || !url.starts_with("https://") {
        return Err("Icon host returned an invalid public image address.".into());
    }
    let expected_worker = format!(
        "{}/icons/{content_hash}.png",
        settings.endpoint.trim_end_matches('/')
    );
    let expected_public = format!(
        "{}/icons/{content_hash}.png",
        settings.public_base.trim_end_matches('/')
    );
    if url != expected_worker && (settings.public_base.is_empty() || url != expected_public) {
        return Err("Icon host returned an image outside the configured public host.".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn configured() -> IconUploadSettings {
        IconUploadSettings {
            enabled: true,
            consent_granted: true,
            endpoint: "https://upload.example.com".into(),
            public_base: "https://icons.example.com".into(),
            credential_stored: false,
        }
    }
    #[test]
    fn upload_defaults_are_off_and_credentials_are_derived() {
        let config: IconUploadSettings =
            serde_json::from_str(r#"{"credentialStored":true}"#).unwrap();
        assert!(!config.enabled);
        assert!(!config.consent_granted);
        assert!(!config.credential_stored);
    }
    #[test]
    fn explicit_consent_and_public_origins_required() {
        let mut settings = configured();
        assert!(settings.validate().is_ok());
        settings.consent_granted = false;
        assert!(settings.validate().is_err());
        for origin in [
            "http://example.com",
            "https://localhost",
            "https://127.0.0.1",
            "https://10.0.0.1",
            "https://user:pass@example.com",
            "https://example.com/path",
            "https://example.com?token=x",
        ] {
            assert!(validate_base(origin).is_err(), "{origin}");
        }
    }
    #[test]
    fn response_cannot_redirect_icon_or_change_digest() {
        let settings = configured();
        let digest = "a".repeat(64);
        assert!(validate_response(
            &settings,
            &digest,
            &format!("https://icons.example.com/icons/{digest}.png"),
            &digest
        )
        .is_ok());
        assert!(validate_response(
            &settings,
            &digest,
            &format!("https://upload.example.com/icons/{digest}.png"),
            &digest
        )
        .is_ok());
        assert!(validate_response(
            &settings,
            &digest,
            &format!("https://evil.example.com/icons/{digest}.png"),
            &digest
        )
        .is_err());
        assert!(validate_response(
            &settings,
            &digest,
            &format!("https://icons.example.com/icons/{digest}.png"),
            &"b".repeat(64)
        )
        .is_err());
    }
    #[test]
    fn upload_cache_is_scoped_to_content_and_host_configuration() {
        let a = configured();
        let mut b = a.clone();
        b.public_base = "https://other.example.com".into();
        assert_ne!(a.fingerprint(), b.fingerprint());
    }
    #[test]
    fn local_path_is_not_upload_content() {
        assert!(png_bytes("C:\\private\\icon.png").is_err());
        assert!(png_bytes("data:image/png;base64,AAAA").is_err());
    }
}
