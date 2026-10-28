mod icons;
#[cfg(windows)]
mod windows;

pub use icons::{DiscordIcon, IconResolver, MappedIconResolver};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedApplication {
    pub id: String,
    pub name: String,
    pub executable_name: String,
    pub executable_path: String,
    pub process_ids: Vec<u32>,
    pub icon_data_url: Option<String>,
    pub running: bool,
}

pub fn normalized_identity(path: &str) -> String {
    path.trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .to_lowercase()
}

pub fn application_id(path: &str) -> String {
    format!("{:x}", Sha256::digest(normalized_identity(path).as_bytes()))
}

pub(crate) fn icon_cache_file(path: &str, cache: &Path) -> Option<std::path::PathBuf> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let key = application_id(&format!("{path}:{}:{modified}", metadata.len()));
    Some(cache.join(format!("{key}.png")))
}

pub fn normalize_name(
    product: Option<&str>,
    description: Option<&str>,
    executable: &str,
) -> String {
    let stem = if executable
        .get(executable.len().saturating_sub(4)..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".exe"))
    {
        &executable[..executable.len() - 4]
    } else {
        executable
    };
    for value in [product, description, Some(stem)].into_iter().flatten() {
        let mut clean = String::new();
        for character in value.chars().filter(|c| !c.is_control()) {
            if clean.len() + character.len_utf8() > 128 {
                break;
            }
            clean.push(character);
        }
        let clean = clean.trim();
        if !clean.is_empty() {
            return clean.to_owned();
        }
    }
    "Application".into()
}

fn deduplicate(apps: Vec<DetectedApplication>) -> Vec<DetectedApplication> {
    let mut unique: BTreeMap<String, DetectedApplication> = BTreeMap::new();
    for app in apps {
        let key = normalized_identity(&app.executable_path);
        if let Some(existing) = unique.get_mut(&key) {
            existing.process_ids.extend(app.process_ids);
            existing.process_ids.sort_unstable();
            existing.process_ids.dedup();
        } else {
            unique.insert(key, app);
        }
    }
    let mut apps: Vec<_> = unique.into_values().collect();
    apps.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    apps
}

pub fn list_running_applications(cache_dir: &Path) -> Result<Vec<DetectedApplication>, String> {
    #[cfg(windows)]
    {
        windows::list(cache_dir).map(deduplicate)
    }
    #[cfg(not(windows))]
    {
        let _ = cache_dir;
        Err("CurrentActivity supports Windows 10 and 11 only".into())
    }
}
pub fn get_foreground_application(cache_dir: &Path) -> Result<Option<DetectedApplication>, String> {
    #[cfg(windows)]
    {
        windows::foreground(cache_dir)
    }
    #[cfg(not(windows))]
    {
        let _ = cache_dir;
        Ok(None)
    }
}
pub fn is_application_running(executable_path: &str) -> bool {
    #[cfg(windows)]
    {
        windows::running(executable_path)
    }
    #[cfg(not(windows))]
    {
        let _ = executable_path;
        false
    }
}
pub fn is_discord_running() -> bool {
    #[cfg(windows)]
    {
        windows::discord_running()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_follow_metadata_priority_and_are_bounded() {
        assert_eq!(
            normalize_name(Some(" \0 "), Some("Editor\n"), "edit.exe"),
            "Editor"
        );
        assert_eq!(normalize_name(None, None, "Code.exe"), "Code");
        assert_eq!(normalize_name(None, None, "Code.EXE"), "Code");
        assert_eq!(
            normalize_name(Some(&"x".repeat(500)), None, "x.exe").len(),
            128
        );
        let unicode = normalize_name(Some(&"界".repeat(100)), None, "x.exe");
        assert!(unicode.len() <= 128);
        assert_eq!(unicode.chars().count(), 42);
    }
    #[test]
    fn identity_is_case_and_separator_independent() {
        assert_eq!(
            application_id("C:/Tools/Code.exe"),
            application_id("c:\\tools\\CODE.EXE")
        );
    }
    #[test]
    fn windows_are_deduplicated_by_executable_not_name() {
        let make = |path: &str, pid| DetectedApplication {
            id: application_id(path),
            name: "Editor".into(),
            executable_name: "editor.exe".into(),
            executable_path: path.into(),
            process_ids: vec![pid],
            icon_data_url: None,
            running: true,
        };
        let apps = deduplicate(vec![
            make("C:/editor.exe", 1),
            make("c:\\EDITOR.exe", 2),
            make("D:/editor.exe", 3),
        ]);
        assert_eq!(apps.len(), 2);
        assert!(apps.iter().any(|a| a.process_ids == [1, 2]));
    }
}
