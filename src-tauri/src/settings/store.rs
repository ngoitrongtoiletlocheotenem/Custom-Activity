use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub start_with_windows: bool,
    pub start_minimized: bool,
    pub automatically_reconnect: bool,
    pub auto_resume: bool,
    pub remember_descriptions: bool,
    pub onboarding_complete: bool,
    pub discord_client_id: String,
    pub icon_upload: crate::icon_upload::IconUploadSettings,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            start_with_windows: false,
            start_minimized: false,
            automatically_reconnect: true,
            auto_resume: false,
            remember_descriptions: true,
            onboarding_complete: false,
            discord_client_id: option_env!("DISCORD_CLIENT_ID").unwrap_or("").to_owned(),
            icon_upload: Default::default(),
        }
    }
}
impl Settings {
    pub fn resolved_client_id(&self) -> Option<String> {
        if !self.discord_client_id.is_empty() {
            Some(self.discord_client_id.clone())
        } else {
            option_env!("DISCORD_CLIENT_ID")
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        self.icon_upload.validate()?;
        if !self.discord_client_id.is_empty()
            && self
                .discord_client_id
                .parse::<u64>()
                .ok()
                .filter(|n| *n != 0)
                .is_none()
        {
            return Err("Discord Application ID must be a positive numeric identifier.".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PresentationOverrides {
    pub application_name_override: String,
    pub details: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SettingsDocument {
    pub settings: Settings,
    /// Opaque executable identity -> explicitly submitted text. Local only.
    pub descriptions: BTreeMap<String, String>,
    /// Explicitly submitted presentation text, governed by Remember descriptions.
    pub presentations: BTreeMap<String, PresentationOverrides>,
}
pub struct SettingsStore {
    pub path: PathBuf,
}
impl SettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn load(&self) -> (SettingsDocument, bool) {
        match fs::read(&self.path) {
            Ok(bytes) => {
                if bytes.len() <= 2 * 1024 * 1024 {
                    if let Ok(mut doc) = serde_json::from_slice::<SettingsDocument>(&bytes) {
                        if doc.settings.validate().is_ok() {
                            doc.descriptions.retain(|key, text| {
                                key.len() == 64
                                    && key.bytes().all(|c| c.is_ascii_hexdigit())
                                    && crate::discord::provider::validate_description(text).is_ok()
                            });
                            doc.presentations.retain(|key, presentation| {
                                key.len() == 64
                                    && key.bytes().all(|c| c.is_ascii_hexdigit())
                                    && (presentation.application_name_override.is_empty()
                                        || crate::discord::provider::validate_activity_name(
                                            &presentation.application_name_override,
                                        )
                                        .is_ok())
                                    && crate::discord::provider::validate_description(
                                        &presentation.details,
                                    )
                                    .is_ok()
                            });
                            return (doc, false);
                        }
                    }
                }
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let _ = fs::rename(
                    &self.path,
                    self.path
                        .with_extension(format!("corrupt-{timestamp}.json")),
                );
                (SettingsDocument::default(), true)
            }
            Err(err) => (
                SettingsDocument::default(),
                err.kind() != std::io::ErrorKind::NotFound,
            ),
        }
    }
    pub fn save(&self, doc: &SettingsDocument) -> Result<(), String> {
        doc.settings.validate()?;
        let parent = self
            .path
            .parent()
            .ok_or("Settings directory is unavailable.")?;
        fs::create_dir_all(parent).map_err(|_| "Cannot create settings directory.")?;
        let data = serde_json::to_vec_pretty(doc).map_err(|_| "Cannot serialize settings.")?;
        let temp = self.path.with_extension("tmp");
        let mut file = fs::File::create(&temp).map_err(|_| "Cannot write settings.")?;
        file.write_all(&data)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot save settings.")?;
        drop(file);
        fs::rename(temp, &self.path).map_err(|_| "Cannot replace settings file.".into())
    }
}
pub fn config_directory() -> PathBuf {
    #[cfg(debug_assertions)]
    if let Some(root) = std::env::var_os("CURRENTACTIVITY_TEST_DATA_DIR") {
        return PathBuf::from(root).join("config");
    }
    dirs::config_dir()
        .unwrap_or_else(|| Path::new(".").to_path_buf())
        .join("CurrentActivity")
}
pub fn local_directory() -> PathBuf {
    #[cfg(debug_assertions)]
    if let Some(root) = std::env::var_os("CURRENTACTIVITY_TEST_DATA_DIR") {
        return PathBuf::from(root).join("local");
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| Path::new(".").to_path_buf())
        .join("CurrentActivity")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_round_trip_and_safe_defaults() {
        let doc = SettingsDocument::default();
        let value = serde_json::to_value(&doc).unwrap();
        let restored: SettingsDocument = serde_json::from_value(value).unwrap();
        assert_eq!(restored.settings, doc.settings);
        assert!(!restored.settings.start_with_windows);
        assert!(!restored.settings.auto_resume);
        assert!(restored.settings.remember_descriptions);
        assert_eq!(
            serde_json::from_str::<Settings>("{}").unwrap(),
            Settings::default()
        );
    }
    #[test]
    fn previous_settings_load_without_presentation_or_session_migration() {
        let previous =
            r#"{"settings":{"rememberDescriptions":true},"descriptions":{"app":"Existing title"}}"#;
        let restored: SettingsDocument = serde_json::from_str(previous).unwrap();
        assert_eq!(restored.descriptions.get("app").unwrap(), "Existing title");
        assert!(restored.presentations.is_empty());
        let serialized = serde_json::to_value(&restored).unwrap();
        assert!(serialized.get("startedAt").is_none());
        assert!(serialized.get("enabled").is_none());
    }
    #[test]
    fn stored_presentations_round_trip_and_invalid_entries_are_filtered() {
        let dir = std::env::temp_dir().join(format!("ca-presentations-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let store = SettingsStore::new(dir.join("settings.json"));
        let mut doc = SettingsDocument::default();
        let valid_key = "a".repeat(64);
        doc.presentations.insert(
            valid_key.clone(),
            PresentationOverrides {
                application_name_override: "Custom game name".into(),
                details: "Chapter two".into(),
            },
        );
        doc.presentations
            .insert("invalid identity".into(), PresentationOverrides::default());
        doc.presentations.insert(
            "b".repeat(64),
            PresentationOverrides {
                application_name_override: "Bad\nname".into(),
                details: String::new(),
            },
        );
        store.save(&doc).unwrap();
        let (restored, recovered) = store.load();
        assert!(!recovered);
        assert_eq!(restored.presentations.len(), 1);
        assert_eq!(
            restored.presentations[&valid_key],
            doc.presentations[&valid_key]
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn corrupt_file_recovers_and_preserves_evidence() {
        let dir = std::env::temp_dir().join(format!("ca-settings-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let store = SettingsStore::new(dir.join("settings.json"));
        fs::write(&store.path, "{broken").unwrap();
        assert!(store.load().1);
        store.save(&SettingsDocument::default()).unwrap();
        assert!(!store.load().1);
        store.save(&SettingsDocument::default()).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
}
