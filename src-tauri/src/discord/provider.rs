use serde::{Deserialize, Serialize};

/// Only manually selected metadata may enter this outbound model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceActivity {
    pub application_name: String,
    pub description: Option<String>,
    pub details: Option<String>,
    /// Unix epoch seconds; converted explicitly to SDK milliseconds by the bridge.
    pub started_at: Option<u64>,
    pub large_image: Option<String>,
    pub large_image_text: Option<String>,
}

pub trait PresenceProvider: Send {
    fn connect(&mut self) -> Result<(), String>;
    fn disconnect(&mut self) -> Result<(), String>;
    fn set_activity(&mut self, activity: &PresenceActivity) -> Result<(), String>;
    fn clear_activity(&mut self) -> Result<(), String>;
    fn is_connected(&self) -> bool;
    fn tick(&mut self) -> Result<(), String> {
        Ok(())
    }
    #[cfg(debug_assertions)]
    fn diagnostics_json(&self) -> Option<String> {
        None
    }
}

/// ActivityAssets::LargeImage is 1-300 characters in SDK 1.10.19337.
/// Only explicitly configured public HTTPS resources or literal asset keys
/// qualify. URL parsing also canonicalizes numeric/encoded IP spellings.
pub fn is_valid_discord_image(image: &str) -> bool {
    if image.is_empty()
        || image.len() > 300
        || image.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return false;
    }
    if image
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return true;
    }
    if !image
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
        || image.contains('\\')
    {
        return false;
    }
    for (index, byte) in image.bytes().enumerate() {
        if byte == b'%'
            && !image
                .as_bytes()
                .get(index + 1..index + 3)
                .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit))
        {
            return false;
        }
    }
    let Ok(parsed) = url::Url::parse(image) else {
        return false;
    };
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || parsed.port().is_some_and(|port| port != 443)
    {
        return false;
    }
    match parsed.host() {
        Some(url::Host::Domain(domain)) => {
            let host = domain.trim_end_matches('.').to_ascii_lowercase();
            host.contains('.')
                && ![
                    "localhost",
                    "local",
                    "lan",
                    "internal",
                    "home",
                    "test",
                    "invalid",
                    "example",
                    "onion",
                ]
                .iter()
                .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
        }
        Some(url::Host::Ipv4(ip)) => public_ipv4(ip),
        Some(url::Host::Ipv6(ip)) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_ipv4(v4);
            }
            let words = ip.segments();
            (words[0] & 0xe000 == 0x2000) && !(words[0] == 0x2001 && words[1] == 0x0db8)
        }
        None => false,
    }
}

fn public_ipv4(ip: std::net::Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && (b == 168 || (b == 0 && (c == 0 || c == 2))))
        || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
        || (a == 203 && b == 0 && c == 113))
}

/// Discord's activity string fields are capped at 128 characters. Enforce the
/// stricter UTF-8 byte limit too, so multi-byte input never exceeds either limit.
pub fn validate_description(value: &str) -> Result<(), String> {
    if !value.is_empty() && value.chars().count() < 2 {
        return Err("Description must be empty or contain at least 2 characters.".into());
    }
    if value.len() > 128 {
        return Err("Description must be at most 128 UTF-8 bytes.".into());
    }
    if value.chars().any(char::is_control) {
        return Err("Description cannot contain control characters.".into());
    }
    Ok(())
}

pub fn validate_activity_name(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(
            "Application name must contain 1–128 UTF-8 bytes without control characters.".into(),
        );
    }
    Ok(())
}

pub fn validate_activity(activity: &PresenceActivity) -> Result<(), String> {
    validate_activity_name(&activity.application_name)?;
    validate_description(activity.description.as_deref().unwrap_or(""))?;
    validate_description(activity.details.as_deref().unwrap_or("")).map_err(|_| {
        "Details must be empty or contain 2–128 UTF-8 bytes without control characters.".to_string()
    })?;
    if activity
        .started_at
        .is_some_and(|value| value == 0 || value > 253_402_300_799)
    {
        return Err("Activity start must be positive Unix seconds before year 10000.".into());
    }
    if activity
        .large_image_text
        .as_ref()
        .is_some_and(|v| v.len() > 128 || v.chars().any(char::is_control))
    {
        return Err("Image label is too long or contains control characters.".into());
    }
    if let Some(image) = &activity.large_image {
        if !is_valid_discord_image(image) {
            return Err("Discord image must be an asset key or HTTPS URL.".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn description_limits_and_empty() {
        assert!(validate_description("").is_ok());
        assert!(validate_description("x").is_err());
        assert!(validate_description("😀").is_err());
        assert!(validate_description("ok").is_ok());
        assert!(validate_description(&"x".repeat(128)).is_ok());
        assert!(validate_description(&"x".repeat(129)).is_err());
        assert!(validate_description(&"😀".repeat(33)).is_err());
        assert!(validate_description("line\nline").is_err());
    }
    #[test]
    fn rejects_local_discord_images() {
        let mut a = PresenceActivity {
            application_name: "Code".into(),
            description: None,
            details: None,
            started_at: None,
            large_image: Some("C:\\private\\icon.png".into()),
            large_image_text: None,
        };
        assert!(validate_activity(&a).is_err());
        a.large_image = Some("currentactivity".into());
        assert!(validate_activity(&a).is_ok());
    }
    #[test]
    fn validates_outbound_names_by_utf8_bytes() {
        let mut a = PresenceActivity {
            application_name: "😀".repeat(32),
            description: None,
            details: None,
            started_at: None,
            large_image: None,
            large_image_text: None,
        };
        assert!(validate_activity(&a).is_ok());
        a.application_name.push('😀');
        assert!(validate_activity(&a).is_err());
        a.application_name = " \t".into();
        assert!(validate_activity(&a).is_err());
        a.application_name = "Editor".into();
        a.description = Some("x".into());
        assert!(validate_activity(&a).is_err());
    }
    #[test]
    fn images_reject_local_private_malformed_and_over_limit_urls() {
        for image in [
            "file:///C:/icon.png",
            "data:image/png;base64,aaa",
            "blob:https://site.com/id",
            "asset://localhost/icon",
            "https://localhost/icon.png",
            "https://127.0.0.1/i",
            "https://2130706433/i",
            "https://0x7f000001/i",
            "https://10.1.2.3/i",
            "https://172.31.1.1/i",
            "https://192.168.1.1/i",
            "https://169.254.1.1/i",
            "https://100.64.0.1/i",
            "https://[::1]/i",
            "https://[fc00::1]/i",
            "https://[::ffff:127.0.0.1]/i",
            "https://nas.local/i",
            "https:///",
            "https://user:password@example.com/i",
            "https://example.com:8443/i",
        ] {
            assert!(!is_valid_discord_image(image), "accepted {image}");
        }
        assert!(!is_valid_discord_image(&format!(
            "https://example.com/{}",
            "x".repeat(300)
        )));
        assert!(is_valid_discord_image("github-desktop"));
        assert!(is_valid_discord_image(
            "https://avatars.githubusercontent.com/u/13171334?s=256"
        ));
    }
    #[test]
    fn secondary_details_and_epoch_seconds_are_validated() {
        let mut activity = PresenceActivity {
            application_name: "Editor".into(),
            description: Some("Title".into()),
            details: Some("Second line".into()),
            started_at: Some(1_793_088_000),
            large_image: None,
            large_image_text: None,
        };
        assert!(validate_activity(&activity).is_ok());
        for bad in ["x".to_owned(), "private\ntext".to_owned(), "界".repeat(43)] {
            activity.details = Some(bad);
            assert!(validate_activity(&activity).is_err());
        }
        activity.details = Some(String::new());
        for bad in [0, 253_402_300_800, u64::MAX] {
            activity.started_at = Some(bad);
            assert!(validate_activity(&activity).is_err());
        }
        activity.started_at = Some(253_402_300_799);
        assert!(validate_activity(&activity).is_ok());
    }
}
