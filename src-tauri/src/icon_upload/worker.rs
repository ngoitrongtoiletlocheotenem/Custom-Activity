use super::{credentials::TokenStore, hash, png_bytes, validate_response, IconUploadSettings};
use crate::applications::DetectedApplication;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

// Deliberately contains no executable path, name, username or description.
struct UploadRequest {
    settings: IconUploadSettings,
    png: Vec<u8>,
    digest: String,
    token: String,
    generation: u64,
}
struct Completion {
    generation: u64,
    result: Result<String, String>,
}
trait UploadExecutor: Send + 'static {
    fn upload(&self, request: &UploadRequest) -> Result<String, String>;
}
struct HttpUploader;
impl UploadExecutor for HttpUploader {
    fn upload(&self, request: &UploadRequest) -> Result<String, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(3))
            .connect_timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| "Icon upload client is unavailable.")?;
        let response = client
            .post(format!(
                "{}/v1/icons",
                request.settings.endpoint.trim_end_matches('/')
            ))
            .bearer_auth(&request.token)
            .header(reqwest::header::CONTENT_TYPE, "image/png")
            .body(request.png.clone())
            .send()
            .map_err(|_| "Icon upload failed; using the configured fallback.")?;
        if !response.status().is_success() {
            return Err("Icon upload was not accepted; using the configured fallback.".into());
        }
        let mut bytes = Vec::new();
        response
            .take(8193)
            .read_to_end(&mut bytes)
            .map_err(|_| "Icon host response was unavailable.")?;
        if bytes.len() > 8192 {
            return Err("Icon host response was too large.".into());
        }
        #[derive(Deserialize)]
        struct Response {
            url: String,
            sha256: String,
        }
        let response: Response = serde_json::from_slice(&bytes)
            .map_err(|_| "Icon host returned an invalid response.")?;
        validate_response(
            &request.settings,
            &request.digest,
            &response.url,
            &response.sha256,
        )?;
        Ok(response.url)
    }
}
#[derive(Serialize, Deserialize)]
struct CacheEntry {
    key: String,
    digest: String,
    url: String,
}
struct Active {
    signature: String,
    settings: IconUploadSettings,
    png: Vec<u8>,
    digest: String,
    url: Option<String>,
    retry_after: Instant,
}
pub struct UploadCoordinator {
    sender: mpsc::SyncSender<UploadRequest>,
    receiver: mpsc::Receiver<Completion>,
    generation: Arc<AtomicU64>,
    current: u64,
    pending: Option<u64>,
    active: Option<Active>,
    cache: Vec<CacheEntry>,
    cache_file: PathBuf,
}
impl UploadCoordinator {
    pub fn new(cache_file: PathBuf) -> Self {
        Self::with_executor(cache_file, HttpUploader)
    }
    fn with_executor(cache_file: PathBuf, executor: impl UploadExecutor) -> Self {
        let (sender, jobs) = mpsc::sync_channel::<UploadRequest>(1);
        let (done, receiver) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let worker_generation = generation.clone();
        let _ = std::thread::Builder::new()
            .name("currentactivity-icon-upload".into())
            .spawn(move || {
                while let Ok(request) = jobs.recv() {
                    if worker_generation.load(Ordering::SeqCst) != request.generation {
                        continue;
                    }
                    let result = executor.upload(&request);
                    if done
                        .send(Completion {
                            generation: request.generation,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
        let cache = if std::fs::metadata(&cache_file).is_ok_and(|m| m.len() <= 256 * 1024) {
            std::fs::read(&cache_file)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Vec<CacheEntry>>(&bytes).ok())
                .unwrap_or_default()
                .into_iter()
                .take(256)
                .collect()
        } else {
            Vec::new()
        };
        Self {
            sender,
            receiver,
            generation,
            current: 0,
            pending: None,
            active: None,
            cache,
            cache_file,
        }
    }
    fn cancel(&mut self) {
        if self.active.is_some() {
            self.current = self.current.wrapping_add(1);
            self.generation.store(self.current, Ordering::SeqCst);
            self.active = None;
            self.pending = None;
        }
    }
    pub fn reset(&mut self) {
        self.cancel();
    }
    /// Called by the presence worker; performs no network request. A stale or
    /// disabled session can never install a completed image on the new selection.
    pub fn step(
        &mut self,
        settings: &IconUploadSettings,
        app: Option<&DetectedApplication>,
        sharing_allowed: bool,
        tokens: &TokenStore,
    ) -> Option<String> {
        let Some(app) =
            app.filter(|_| sharing_allowed && settings.enabled && settings.consent_granted)
        else {
            self.cancel();
            return None;
        };
        if settings.validate().is_err() {
            self.cancel();
            return None;
        }
        let Some(data) = app.icon_data_url.as_deref() else {
            self.cancel();
            return None;
        };
        let signature = hash(
            format!(
                "{}:{}:{}",
                app.id,
                settings.fingerprint(),
                hash(data.as_bytes())
            )
            .as_bytes(),
        );
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.signature != signature)
        {
            self.cancel();
            self.current = self.current.wrapping_add(1);
            self.generation.store(self.current, Ordering::SeqCst);
            let png = png_bytes(data).ok()?;
            let digest = hash(&png);
            let key = format!("{}:{digest}", settings.fingerprint());
            let url = self
                .cache
                .iter()
                .find(|entry| {
                    entry.key == key
                        && validate_response(settings, &digest, &entry.url, &entry.digest).is_ok()
                })
                .map(|entry| entry.url.clone());
            self.active = Some(Active {
                signature,
                settings: settings.clone(),
                png,
                digest,
                url,
                retry_after: Instant::now(),
            });
        }
        while let Ok(completed) = self.receiver.try_recv() {
            if completed.generation != self.current {
                continue;
            }
            self.pending = None;
            if let Some(active) = self.active.as_mut() {
                match completed.result {
                    Ok(url) => {
                        let key = format!("{}:{}", active.settings.fingerprint(), active.digest);
                        self.cache.retain(|entry| entry.key != key);
                        self.cache.push(CacheEntry {
                            key,
                            digest: active.digest.clone(),
                            url: url.clone(),
                        });
                        if self.cache.len() > 256 {
                            self.cache.drain(..self.cache.len() - 256);
                        }
                        active.url = Some(url);
                        if let Some(parent) = self.cache_file.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Ok(bytes) = serde_json::to_vec(&self.cache) {
                            let _ = std::fs::write(&self.cache_file, bytes);
                        }
                    }
                    Err(_) => active.retry_after = Instant::now() + Duration::from_secs(60),
                }
            }
        }
        let active = self.active.as_mut()?;
        if active.url.is_none() && self.pending.is_none() && Instant::now() >= active.retry_after {
            if let Ok(token) = tokens.load(&settings.endpoint) {
                let request = UploadRequest {
                    settings: active.settings.clone(),
                    png: active.png.clone(),
                    digest: active.digest.clone(),
                    token,
                    generation: self.current,
                };
                if self.sender.try_send(request).is_ok() {
                    self.pending = Some(self.current);
                }
            } else {
                active.retry_after = Instant::now() + Duration::from_secs(60);
            }
        }
        active.url.clone()
    }
}
impl Drop for UploadCoordinator {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct MockUploader {
        calls: Arc<AtomicU64>,
    }
    impl UploadExecutor for MockUploader {
        fn upload(&self, request: &UploadRequest) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request.png, b"png-bytes-only");
            Ok(format!(
                "{}/icons/{}.png",
                request.settings.public_base, request.digest
            ))
        }
    }
    #[test]
    fn transport_contract_only_transmits_supplied_png_and_content_identity() {
        let calls = Arc::new(AtomicU64::new(0));
        let mock = MockUploader {
            calls: calls.clone(),
        };
        let request = UploadRequest {
            settings: IconUploadSettings {
                endpoint: "https://upload.example.com".into(),
                public_base: "https://icons.example.com".into(),
                ..Default::default()
            },
            png: b"png-bytes-only".to_vec(),
            digest: hash(b"png-bytes-only"),
            token: "not-a-real-token-12345678901234567890".into(),
            generation: 1,
        };
        assert!(mock
            .upload(&request)
            .unwrap()
            .ends_with(&format!("{}.png", request.digest)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn disabled_consent_never_schedules_even_with_an_icon() {
        let calls = Arc::new(AtomicU64::new(0));
        let path = std::env::temp_dir().join(format!("ca-upload-test-{}", std::process::id()));
        let mut coordinator = UploadCoordinator::with_executor(
            path,
            MockUploader {
                calls: calls.clone(),
            },
        );
        let app = DetectedApplication {
            id: "local-only".into(),
            name: "Private Name".into(),
            executable_name: "private.exe".into(),
            executable_path: "C:/private/user/app.exe".into(),
            process_ids: vec![1],
            icon_data_url: Some("data:image/png;base64,AAAA".into()),
            running: true,
        };
        assert!(coordinator
            .step(
                &IconUploadSettings::default(),
                Some(&app),
                true,
                &TokenStore::new("nonexistent")
            )
            .is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(coordinator.active.is_none());
    }
    #[test]
    fn cancellation_invalidates_inflight_generation() {
        let path = std::env::temp_dir().join("ca-unused-upload-cache");
        let mut coordinator = UploadCoordinator::with_executor(
            path,
            MockUploader {
                calls: Arc::new(AtomicU64::new(0)),
            },
        );
        coordinator.current = 7;
        coordinator.generation.store(7, Ordering::SeqCst);
        coordinator.active = Some(Active {
            signature: "a".into(),
            settings: Default::default(),
            png: vec![],
            digest: "a".into(),
            url: None,
            retry_after: Instant::now(),
        });
        coordinator.cancel();
        assert_eq!(coordinator.generation.load(Ordering::SeqCst), 8);
        assert!(coordinator.active.is_none());
    }

    #[cfg(windows)]
    struct ResultUploader {
        calls: Arc<AtomicU64>,
        fail: bool,
    }
    #[cfg(windows)]
    impl UploadExecutor for ResultUploader {
        fn upload(&self, request: &UploadRequest) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request.digest, hash(&request.png));
            assert!(
                image::load_from_memory_with_format(&request.png, image::ImageFormat::Png).is_ok()
            );
            if self.fail {
                Err("mock outage".into())
            } else {
                Ok(format!(
                    "{}/icons/{}.png",
                    request.settings.public_base, request.digest
                ))
            }
        }
    }
    #[cfg(windows)]
    fn consented_fixture() -> (PathBuf, IconUploadSettings, DetectedApplication, TokenStore) {
        use base64::Engine;
        let directory = std::env::temp_dir().join(format!(
            "ca-upload-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let settings = IconUploadSettings {
            enabled: true,
            consent_granted: true,
            endpoint: "https://upload.example.com".into(),
            public_base: "https://icons.example.com".into(),
            credential_stored: true,
        };
        let tokens = TokenStore::new(directory.join("token.dpapi"));
        tokens
            .save(&settings.endpoint, "test-only-token-12345678901234567890")
            .unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(64, 64, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let data = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png.into_inner())
        );
        let app = DetectedApplication {
            id: "opaque-local-selection".into(),
            name: "Private Name".into(),
            executable_name: "private.exe".into(),
            executable_path: "C:/private/user/app.exe".into(),
            process_ids: vec![9],
            icon_data_url: Some(data),
            running: true,
        };
        (directory, settings, app, tokens)
    }
    #[cfg(windows)]
    #[test]
    fn background_success_is_cached_by_content_without_selection_metadata() {
        let (directory, settings, app, tokens) = consented_fixture();
        let path = directory.join("cache.json");
        let calls = Arc::new(AtomicU64::new(0));
        let mut coordinator = UploadCoordinator::with_executor(
            path.clone(),
            ResultUploader {
                calls: calls.clone(),
                fail: false,
            },
        );
        assert!(coordinator
            .step(&settings, Some(&app), true, &tokens)
            .is_none());
        let deadline = Instant::now() + Duration::from_secs(2);
        let url = loop {
            if let Some(url) = coordinator.step(&settings, Some(&app), true, &tokens) {
                break url;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("Private"));
        assert!(!saved.contains("private.exe"));
        assert!(!saved.contains("test-only-token"));
        coordinator.reset();
        assert_eq!(
            coordinator.step(&settings, Some(&app), true, &tokens),
            Some(url)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(coordinator
            .step(&settings, Some(&app), false, &tokens)
            .is_none());
        drop(coordinator);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(directory.join("token.dpapi")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn background_failure_returns_no_override_and_backs_off() {
        let (directory, settings, app, tokens) = consented_fixture();
        let calls = Arc::new(AtomicU64::new(0));
        let mut coordinator = UploadCoordinator::with_executor(
            directory.join("cache.json"),
            ResultUploader {
                calls: calls.clone(),
                fail: true,
            },
        );
        assert!(coordinator
            .step(&settings, Some(&app), true, &tokens)
            .is_none());
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            assert!(coordinator
                .step(&settings, Some(&app), true, &tokens)
                .is_none());
            if calls.load(Ordering::SeqCst) == 1 && coordinator.pending.is_none() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        for _ in 0..10 {
            assert!(coordinator
                .step(&settings, Some(&app), true, &tokens)
                .is_none());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(coordinator);
        std::fs::remove_file(directory.join("token.dpapi")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
