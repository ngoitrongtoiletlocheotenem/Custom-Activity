use super::DetectedApplication;
use crate::discord::provider::is_valid_discord_image;
use serde::{Deserialize, Serialize};

/// Local preview bytes never become a Discord image. Only registry configuration
/// is permitted across that boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordIcon {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord_image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord_asset_key: Option<String>,
    pub source: String,
}
impl DiscordIcon {
    pub fn image(&self) -> Option<String> {
        self.discord_asset_key
            .clone()
            .or_else(|| self.discord_image_url.clone())
    }
}
pub trait IconResolver: Send + Sync {
    fn resolve(&self, app: &DetectedApplication) -> DiscordIcon;
}
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ImageConfiguration {
    discord_asset_key: Option<String>,
    discord_image_url: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Mapping {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    display_name: String,
    #[serde(default)]
    executables: Vec<String>,
    #[serde(default)]
    product_names: Vec<String>,
    #[serde(flatten)]
    image: ImageConfiguration,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registry {
    schema_version: u32,
    #[serde(default)]
    fallback: ImageConfiguration,
    applications: Vec<Mapping>,
}
pub struct MappedIconResolver {
    registry: Registry,
}
impl Default for MappedIconResolver {
    fn default() -> Self {
        Self::from_json(include_str!("../../../assets/app-icons.json")).unwrap_or_else(|_| Self {
            registry: Registry {
                schema_version: 2,
                fallback: ImageConfiguration::default(),
                applications: Vec::new(),
            },
        })
    }
}
impl MappedIconResolver {
    /// Registry values are explicit operator configuration. Syntax validation
    /// cannot prove a key was uploaded into the selected Discord Application ID.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let registry: Registry =
            serde_json::from_str(json).map_err(|_| "Invalid icon registry JSON.")?;
        if registry.schema_version != 2 {
            return Err("Unsupported icon registry schema.".into());
        }
        Ok(Self { registry })
    }
    fn configured(image: &ImageConfiguration, fallback: bool) -> Option<DiscordIcon> {
        if let Some(key) = image
            .discord_asset_key
            .as_ref()
            .filter(|key| !key.contains("://") && is_valid_discord_image(key))
        {
            return Some(DiscordIcon {
                discord_asset_key: Some(key.clone()),
                discord_image_url: None,
                source: if fallback {
                    "fallback"
                } else {
                    "discord_asset"
                }
                .into(),
            });
        }
        if let Some(url) = image
            .discord_image_url
            .as_ref()
            .filter(|url| url.starts_with("https://") && is_valid_discord_image(url))
        {
            return Some(DiscordIcon {
                discord_asset_key: None,
                discord_image_url: Some(url.clone()),
                source: if fallback { "fallback" } else { "external_url" }.into(),
            });
        }
        None
    }
}
impl IconResolver for MappedIconResolver {
    fn resolve(&self, app: &DetectedApplication) -> DiscordIcon {
        // Executable identity takes precedence over product metadata collisions.
        let mapping = self
            .registry
            .applications
            .iter()
            .find(|mapping| {
                mapping
                    .executables
                    .iter()
                    .any(|exe| exe.eq_ignore_ascii_case(&app.executable_name))
            })
            .or_else(|| {
                self.registry.applications.iter().find(|mapping| {
                    mapping
                        .product_names
                        .iter()
                        .any(|name| name.trim().eq_ignore_ascii_case(app.name.trim()))
                })
            });
        if let Some(icon) = mapping.and_then(|mapping| Self::configured(&mapping.image, false)) {
            return icon;
        }
        Self::configured(&self.registry.fallback, true).unwrap_or(DiscordIcon {
            discord_image_url: None,
            discord_asset_key: None,
            source: "fallback".into(),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn app(exe: &str) -> DetectedApplication {
        DetectedApplication {
            id: "id".into(),
            name: "App".into(),
            executable_name: exe.into(),
            executable_path: "C:\\private.exe".into(),
            process_ids: vec![1],
            icon_data_url: Some("data:image/png;base64,private".into()),
            running: true,
        }
    }
    fn registry(image: serde_json::Value, fallback: serde_json::Value) -> MappedIconResolver {
        let mut mapping = serde_json::json!({"id":"github-desktop","displayName":"GitHub Desktop","executables":["GitHubDesktop.exe"],"productNames":["GitHub Desktop"]});
        mapping
            .as_object_mut()
            .unwrap()
            .extend(image.as_object().unwrap().clone());
        MappedIconResolver::from_json(
            &serde_json::json!({"schemaVersion":2,"fallback":fallback,"applications":[mapping]})
                .to_string(),
        )
        .unwrap()
    }
    #[test]
    fn github_matches_executable_case_insensitively() {
        let result = MappedIconResolver::default().resolve(&app("GITHUBDESKTOP.EXE"));
        assert_eq!(result.source, "external_url");
        assert_eq!(
            result.image().as_deref(),
            Some("https://avatars.githubusercontent.com/u/13171334?s=256")
        );
    }
    #[test]
    fn github_matches_product_name_without_known_executable() {
        let mut application = app("custom-launcher.exe");
        application.name = "github desktop".into();
        assert_eq!(
            MappedIconResolver::default().resolve(&application).source,
            "external_url"
        );
    }
    #[test]
    fn exact_configured_asset_wins_over_url() {
        let resolver = registry(
            serde_json::json!({"discordAssetKey":"github-desktop","discordImageUrl":"https://avatars.githubusercontent.com/u/13171334?s=256"}),
            serde_json::json!({}),
        );
        let result = resolver.resolve(&app("GitHubDesktop.exe"));
        assert_eq!(result.source, "discord_asset");
        assert_eq!(result.image().as_deref(), Some("github-desktop"));
        assert!(result.discord_image_url.is_none());
    }
    #[test]
    fn invalid_asset_tries_valid_url() {
        let resolver = registry(
            serde_json::json!({"discordAssetKey":"C:\\private.png","discordImageUrl":"https://avatars.githubusercontent.com/u/13171334?s=256"}),
            serde_json::json!({}),
        );
        assert_eq!(
            resolver.resolve(&app("GitHubDesktop.exe")).source,
            "external_url"
        );
    }
    #[test]
    fn invalid_local_or_private_images_use_fallback() {
        for invalid in [
            "C:\\test\\icon.png",
            "file:///C:/icon.png",
            "data:image/png;base64,abc",
            "blob:https://example.com/id",
            "https://localhost/icon.png",
            "https://127.0.0.1/icon.png",
            "https://192.168.1.1/icon.png",
        ] {
            let resolver = registry(
                serde_json::json!({"discordImageUrl":invalid}),
                serde_json::json!({"discordAssetKey":"currentactivity-default"}),
            );
            let result = resolver.resolve(&app("GitHubDesktop.exe"));
            assert_eq!(result.source, "fallback");
            assert_eq!(result.image().as_deref(), Some("currentactivity-default"));
        }
    }
    #[test]
    fn invalid_fallback_omits_image_without_using_local_preview() {
        let resolver = registry(
            serde_json::json!({}),
            serde_json::json!({"discordAssetKey":"C:\\bad.png","discordImageUrl":"https://10.0.0.1/private.png"}),
        );
        let result = resolver.resolve(&app("unknown.exe"));
        assert!(result.image().is_none());
        assert!(!serde_json::to_string(&result).unwrap().contains("private"));
    }
    #[test]
    fn unknown_uses_verified_public_generic_computer_image() {
        let result = MappedIconResolver::default().resolve(&app("unknown.exe"));
        assert_eq!(result.source, "fallback");
        assert_eq!(result.image().as_deref(), Some("https://raw.githubusercontent.com/microsoft/fluentui-emoji/1ffb34c752ecf5d402f04cfb4b392c77f57c54bc/assets/Desktop%20computer/3D/desktop_computer_3d.png"));
        assert!(result.discord_asset_key.is_none());
    }
    #[test]
    fn required_categories_are_retained_without_invented_uploads() {
        let resolver = MappedIconResolver::default();
        assert!(resolver.registry.applications.len() >= 18);
        assert!(resolver
            .registry
            .applications
            .iter()
            .all(|app| app.image.discord_asset_key.is_none()));
    }
    #[test]
    fn unknown_schema_is_rejected() {
        assert!(MappedIconResolver::from_json(r#"{"schemaVersion":1,"applications":[]}"#).is_err());
    }
}
