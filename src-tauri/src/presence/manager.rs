use crate::{
    applications::{DetectedApplication, IconResolver, MappedIconResolver},
    discord::{PresenceActivity, PresenceProvider},
    settings::{Settings, SettingsDocument},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionState {
    pub status: String,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityState {
    pub enabled: bool,
    pub selected_application: Option<DetectedApplication>,
    pub description: String,
    pub discord_state: ConnectionState,
    pub selected_application_running: bool,
    pub resume_required: bool,
    pub published: bool,
}
impl Default for ActivityState {
    fn default() -> Self {
        Self {
            enabled: false,
            selected_application: None,
            description: String::new(),
            discord_state: ConnectionState {
                status: "disabled".into(),
                message: "Activity disabled".into(),
            },
            selected_application_running: false,
            resume_required: false,
            published: false,
        }
    }
}
pub struct Manager {
    pub state: ActivityState,
    pub document: SettingsDocument,
    provider: Box<dyn PresenceProvider>,
    resolver: MappedIconResolver,
    last_activity: Option<PresenceActivity>,
    next_attempt: u64,
    failures: usize,
    attempted: bool,
    uploaded_icon: Option<(String, String)>,
}
impl Manager {
    pub fn new(provider: Box<dyn PresenceProvider>, document: SettingsDocument) -> Self {
        Self {
            state: ActivityState::default(),
            document,
            provider,
            resolver: MappedIconResolver::default(),
            last_activity: None,
            next_attempt: 0,
            failures: 0,
            attempted: false,
            uploaded_icon: None,
        }
    }
    fn connection(&mut self, status: &str, message: impl Into<String>) {
        self.state.discord_state = ConnectionState {
            status: status.into(),
            message: message.into(),
        };
    }
    fn clear(&mut self) -> Result<(), String> {
        self.last_activity = None;
        self.state.published = false;
        if let Err(error) = self.provider.clear_activity() {
            // Dropping the SDK client also releases its IPC activity if clearing failed.
            let _ = self.provider.disconnect();
            return Err(error);
        }
        Ok(())
    }
    pub fn select(&mut self, app: DetectedApplication) -> Result<(), String> {
        self.clear()?;
        self.uploaded_icon = None;
        self.state.description = if self.document.settings.remember_descriptions {
            self.document
                .descriptions
                .get(&app.id)
                .cloned()
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.state.selected_application_running = app.running;
        self.state.selected_application = Some(app);
        self.state.resume_required = false;
        self.next_attempt = 0;
        Ok(())
    }
    pub fn set_presence(&mut self, app_id: &str, description: String) -> Result<(), String> {
        crate::discord::provider::validate_description(&description)?;
        let app = self
            .state
            .selected_application
            .as_ref()
            .filter(|app| app.id == app_id)
            .ok_or("Select a running application first.")?;
        if !self.state.selected_application_running {
            return Err("Selected application is no longer running.".into());
        }
        if self.document.settings.remember_descriptions {
            self.document
                .descriptions
                .insert(app.id.clone(), description.clone());
        }
        self.state.description = description;
        self.state.resume_required = false;
        // An explicit user action permits one connection attempt even with auto-reconnect off.
        self.attempted = false;
        self.next_attempt = 0;
        Ok(())
    }
    pub fn submit_presence(
        &mut self,
        app_id: &str,
        description: String,
        persist: impl FnOnce(&SettingsDocument) -> Result<(), String>,
    ) -> Result<(), String> {
        let previous_state = self.state.clone();
        let previous_document = self.document.clone();
        self.set_presence(app_id, description)?;
        if let Err(error) = persist(&self.document) {
            self.state = previous_state;
            self.document = previous_document;
            return Err(error);
        }
        Ok(())
    }
    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if enabled && !self.document.settings.onboarding_complete {
            return Err("Read the privacy introduction before enabling sharing.".into());
        }
        if enabled && !self.state.selected_application_running {
            return Err("Select a running application before enabling sharing.".into());
        }
        self.state.enabled = enabled;
        if !enabled {
            self.uploaded_icon = None;
            let result = self.clear();
            let _ = self.provider.disconnect();
            self.connection("disabled", "Activity disabled");
            result
        } else {
            self.state.resume_required = false;
            self.attempted = false;
            self.next_attempt = 0;
            Ok(())
        }
    }
    pub fn replace_provider(&mut self, provider: Box<dyn PresenceProvider>) {
        // A broken old connection must not prevent adopting the newly saved ID.
        // Disconnect/drop release the old SDK handle even if its clear failed.
        let _ = self.clear();
        let _ = self.provider.disconnect();
        self.provider = provider;
        self.attempted = false;
        self.next_attempt = 0;
    }
    pub fn update_settings(&mut self, settings: Settings) -> Result<(), String> {
        settings.validate()?;
        if !settings.remember_descriptions {
            self.document.descriptions.clear();
        }
        if settings.automatically_reconnect && !self.document.settings.automatically_reconnect {
            self.attempted = false;
            self.next_attempt = 0;
        }
        self.document.settings = settings;
        Ok(())
    }
    /// Only a generation-checked selected-icon upload may supply this override.
    /// It is never restored from settings or accepted through the renderer.
    pub fn set_uploaded_icon(&mut self, app_id: &str, url: Option<String>) {
        self.uploaded_icon = url
            .filter(|url| {
                self.state.enabled
                    && self.document.settings.icon_upload.enabled
                    && self.document.settings.icon_upload.consent_granted
                    && self.state.selected_application_running
                    && !self.state.resume_required
                    && self
                        .state
                        .selected_application
                        .as_ref()
                        .is_some_and(|app| app.id == app_id)
                    && url.starts_with("https://")
                    && crate::discord::provider::is_valid_discord_image(url)
            })
            .map(|url| (app_id.to_owned(), url));
    }
    fn discord_icon_image(&self, app: &DetectedApplication) -> Option<String> {
        self.uploaded_icon
            .as_ref()
            .filter(|(id, _)| {
                id == &app.id
                    && self.document.settings.icon_upload.enabled
                    && self.document.settings.icon_upload.consent_granted
            })
            .map(|(_, url)| url.clone())
            .or_else(|| self.resolver.resolve(app).image())
    }
    #[cfg(debug_assertions)]
    pub fn icon_diagnostics(&self, cache: &std::path::Path) -> serde_json::Value {
        let app = self.state.selected_application.as_ref();
        let icon = app.map(|app| self.resolver.resolve(app));
        let native: Option<serde_json::Value> = self
            .provider
            .diagnostics_json()
            .and_then(|json| serde_json::from_str(&json).ok());
        serde_json::json!({
            "selectedApplication": app.map(|app| serde_json::json!({"name":app.name,"executableName":app.executable_name})),
            "localIcon": {
                "available":app.is_some_and(|app| app.icon_data_url.is_some()),
                "cachePath":app.and_then(|app| crate::applications::icon_cache_file(&app.executable_path, cache))
                    .filter(|path| path.is_file()).map(|path| path.to_string_lossy().into_owned())
            },
            "resolvedIcon": icon.map(|icon| serde_json::json!({"discordImage":app.and_then(|app|self.discord_icon_image(app)),"source":if self.uploaded_icon.is_some(){"uploaded"}else{&icon.source}})),
            "native":native.map(|raw| serde_json::json!({
                "assetsCreated":raw["assetsCreated"], "setAssetsCalled":raw["setAssetsCalled"],
                "assetsAttached":raw["assetsAttached"], "largeImage":raw["finalLargeImage"],
                "largeText":raw["finalLargeText"], "callbackReceived":raw["updateCallbackReceived"],
                "callbackSuccessful":raw["updateCallbackSuccessful"], "callbackErrorCode":raw["updateCallbackErrorCode"],
                "updateResult":raw["updateResult"]
            }))
        })
    }
    fn fail(&mut self, error: String, now: u64) {
        self.state.published = false;
        self.last_activity = None;
        let _ = self.provider.disconnect();
        let delays = [1000, 2000, 5000, 10000, 15000];
        self.next_attempt = now + delays[self.failures.min(delays.len() - 1)];
        self.failures += 1;
        self.connection("error", error);
    }
    /// Worker invokes this with monotonic milliseconds; core tests inject time and process state.
    pub fn tick(&mut self, discord_running: bool, app_running: bool, now: u64) {
        if let Err(error) = self.provider.tick() {
            self.fail(error, now);
        }
        let was_running = self.state.selected_application_running;
        self.state.selected_application_running =
            self.state.selected_application.is_some() && app_running;
        if let Some(app) = self.state.selected_application.as_mut() {
            app.running = self.state.selected_application_running;
        }
        if was_running && !app_running {
            self.state.resume_required = !self.document.settings.auto_resume;
            if let Err(error) = self.clear() {
                self.connection("error", error);
            }
        }
        if !self.state.enabled {
            self.connection("disabled", "Activity disabled");
            return;
        }
        if !discord_running {
            let _ = self.clear();
            let _ = self.provider.disconnect();
            self.connection(
                "waiting",
                if self.document.settings.automatically_reconnect {
                    "Waiting for Discord"
                } else {
                    "Waiting for Discord. Toggle sharing or update to reconnect."
                },
            );
            self.failures = 0;
            if self.document.settings.automatically_reconnect {
                self.attempted = false;
                self.next_attempt = 0;
            }
            return;
        }
        if !self.provider.is_connected() {
            self.state.published = false;
            self.last_activity = None;
            if now < self.next_attempt
                || (self.attempted && !self.document.settings.automatically_reconnect)
            {
                return;
            }
            self.attempted = true;
            match self.provider.connect() {
                Ok(()) if self.provider.is_connected() => {
                    self.failures = 0;
                    self.next_attempt = 0;
                }
                Ok(()) => {
                    self.fail("Waiting for the Discord desktop connection.".into(), now);
                    return;
                }
                Err(error) => {
                    self.fail(error, now);
                    return;
                }
            }
        }
        self.connection("connected", "Connected to Discord");
        if !self.state.selected_application_running || self.state.resume_required {
            return;
        }
        let Some(app) = &self.state.selected_application else {
            return;
        };
        let activity = PresenceActivity {
            application_name: app.name.clone(),
            description: if self.state.description.is_empty() {
                None
            } else {
                Some(self.state.description.clone())
            },
            large_image: self.discord_icon_image(app),
            large_image_text: Some(app.name.clone()),
        };
        if self.last_activity.as_ref() != Some(&activity) {
            #[cfg(debug_assertions)]
            eprintln!(
                "[CurrentActivity icon resolver] {}",
                serde_json::json!({
                    "executable":app.executable_name,"applicationName":app.name,
                    "localIconAvailable":app.icon_data_url.is_some(),
                    "discordImage":activity.large_image,
                    "source":if self.uploaded_icon.is_some(){"uploaded".to_owned()}else{self.resolver.resolve(app).source}
                })
            );
            match self.provider.set_activity(&activity) {
                Ok(()) => {
                    self.last_activity = Some(activity);
                    self.state.published = true;
                }
                Err(error) => self.fail(error, now),
            }
        }
    }
    pub fn shutdown(&mut self) {
        self.state.enabled = false;
        let _ = self.clear();
        let _ = self.provider.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    #[derive(Default)]
    struct Calls {
        sets: Vec<PresenceActivity>,
        clears: usize,
        connects: usize,
        fail: bool,
        clear_fails: bool,
    }
    struct Mock {
        calls: Arc<Mutex<Calls>>,
        connected: bool,
    }
    impl PresenceProvider for Mock {
        fn connect(&mut self) -> Result<(), String> {
            let mut c = self.calls.lock().unwrap();
            c.connects += 1;
            if c.fail {
                return Err("Unavailable".into());
            }
            self.connected = true;
            Ok(())
        }
        fn disconnect(&mut self) -> Result<(), String> {
            self.connected = false;
            Ok(())
        }
        fn set_activity(&mut self, a: &PresenceActivity) -> Result<(), String> {
            self.calls.lock().unwrap().sets.push(a.clone());
            Ok(())
        }
        fn clear_activity(&mut self) -> Result<(), String> {
            let mut calls = self.calls.lock().unwrap();
            calls.clears += 1;
            if calls.clear_fails {
                Err("Clear unavailable".into())
            } else {
                Ok(())
            }
        }
        fn is_connected(&self) -> bool {
            self.connected
        }
    }
    fn setup() -> (Manager, Arc<Mutex<Calls>>) {
        let calls = Arc::new(Mutex::new(Calls::default()));
        let mut doc = SettingsDocument::default();
        doc.settings.onboarding_complete = true;
        let mut m = Manager::new(
            Box::new(Mock {
                calls: calls.clone(),
                connected: false,
            }),
            doc,
        );
        m.select(DetectedApplication {
            id: "a".repeat(64),
            name: "Editor".into(),
            executable_name: "unknown.exe".into(),
            executable_path: "C:/private/project/editor.exe".into(),
            process_ids: vec![42],
            icon_data_url: None,
            running: true,
        })
        .unwrap();
        (m, calls)
    }
    #[test]
    fn sharing_is_explicit_and_updates_only_when_changed() {
        let (mut m, c) = setup();
        m.tick(true, true, 0);
        assert_eq!(c.lock().unwrap().connects, 0);
        m.set_presence(&"a".repeat(64), "Building".into()).unwrap();
        m.set_enabled(true).unwrap();
        m.tick(true, true, 0);
        m.tick(true, true, 2000);
        assert_eq!(c.lock().unwrap().sets.len(), 1);
        m.set_enabled(false).unwrap();
        assert!(!m.state.published);
        assert!(!m.state.enabled);
    }
    #[test]
    fn closing_stops_and_default_restart_requires_user() {
        let (mut m, c) = setup();
        m.set_enabled(true).unwrap();
        m.tick(true, true, 0);
        m.tick(true, false, 1500);
        assert!(!m.state.published);
        assert!(m.state.resume_required);
        m.tick(true, true, 3000);
        assert_eq!(c.lock().unwrap().sets.len(), 1);
        m.set_presence(&"a".repeat(64), "".into()).unwrap();
        m.tick(true, true, 3100);
        assert_eq!(c.lock().unwrap().sets.len(), 2);
    }
    #[test]
    fn opted_in_auto_resume_restores() {
        let (mut m, c) = setup();
        m.document.settings.auto_resume = true;
        m.set_enabled(true).unwrap();
        m.tick(true, true, 0);
        m.tick(true, false, 1500);
        m.tick(true, true, 3000);
        assert_eq!(c.lock().unwrap().sets.len(), 2);
    }
    #[test]
    fn reconnect_restores_and_backoff_is_bounded() {
        let (mut m, c) = setup();
        m.set_enabled(true).unwrap();
        c.lock().unwrap().fail = true;
        for time in [0, 100, 999, 1000, 2000, 3000, 7999, 8000] {
            m.tick(true, true, time);
        }
        assert_eq!(c.lock().unwrap().connects, 4);
        c.lock().unwrap().fail = false;
        m.tick(false, true, 8100);
        assert_eq!(m.state.discord_state.status, "waiting");
        m.tick(true, true, 8200);
        assert!(m.state.published);
        m.tick(false, true, 9000);
        assert!(!m.state.published);
        m.tick(true, true, 10000);
        assert!(m.state.published);
    }
    #[test]
    fn reconnect_preference_off_is_respected() {
        let (mut m, c) = setup();
        m.document.settings.automatically_reconnect = false;
        c.lock().unwrap().fail = true;
        m.set_enabled(true).unwrap();
        m.tick(true, true, 0);
        m.tick(true, true, 20000);
        assert_eq!(c.lock().unwrap().connects, 1);
    }
    #[test]
    fn outbound_model_excludes_local_metadata() {
        let (mut m, c) = setup();
        m.set_enabled(true).unwrap();
        m.tick(true, true, 0);
        let json = serde_json::to_string(&c.lock().unwrap().sets[0]).unwrap();
        assert!(!json.contains("private"));
        assert!(!json.contains("executable"));
        assert!(!json.contains("42"));
        assert!(c.lock().unwrap().sets[0]
            .large_image
            .as_deref()
            .is_some_and(|image| image.starts_with("https://")));
    }
    #[test]
    fn switching_restores_only_explicit_descriptions() {
        let (mut m, _) = setup();
        m.set_presence(&"a".repeat(64), "Hello".into()).unwrap();
        let app = m.state.selected_application.clone().unwrap();
        m.select(app).unwrap();
        assert_eq!(m.state.description, "Hello");
        m.document.settings.remember_descriptions = false;
        let app = m.state.selected_application.clone().unwrap();
        m.select(app).unwrap();
        assert!(m.state.description.is_empty());
    }
    #[test]
    fn failed_persistence_does_not_publish_or_remember_new_text() {
        let (mut m, calls) = setup();
        m.set_presence(&"a".repeat(64), "Original".into()).unwrap();
        m.set_enabled(true).unwrap();
        m.tick(true, true, 0);
        assert!(m
            .submit_presence(&"a".repeat(64), "Replacement".into(), |_| Err(
                "Disk full".into()
            ))
            .is_err());
        m.tick(true, true, 100);
        assert_eq!(m.state.description, "Original");
        assert_eq!(m.document.descriptions[&"a".repeat(64)], "Original");
        assert_eq!(calls.lock().unwrap().sets.len(), 1);
    }
    #[test]
    fn changing_provider_recovers_even_when_old_connection_cannot_clear() {
        let (mut m, old_calls) = setup();
        m.set_enabled(true).unwrap();
        m.tick(true, true, 0);
        old_calls.lock().unwrap().clear_fails = true;
        let new_calls = Arc::new(Mutex::new(Calls::default()));
        m.replace_provider(Box::new(Mock {
            calls: new_calls.clone(),
            connected: false,
        }));
        assert!(!m.state.published);
        m.tick(true, true, 100);
        assert_eq!(new_calls.lock().unwrap().sets.len(), 1);
    }
    #[test]
    fn disabling_and_shutdown_fail_closed_when_clear_errors() {
        let (mut m, calls) = setup();
        m.set_enabled(true).unwrap();
        m.tick(true, true, 0);
        calls.lock().unwrap().clear_fails = true;
        assert!(m.set_enabled(false).is_err());
        assert!(!m.state.enabled);
        assert!(!m.state.published);
        assert!(!m.provider.is_connected());
        m.shutdown();
        assert!(!m.state.enabled);
        assert!(!m.state.published);
    }

    #[test]
    fn uploaded_icon_updates_only_image_and_stale_selection_is_rejected() {
        let (mut m, calls) = setup();
        m.set_presence(&"a".repeat(64), "Explicit description".into())
            .unwrap();
        m.document.settings.icon_upload.enabled = true;
        m.document.settings.icon_upload.consent_granted = true;
        m.set_enabled(true).unwrap();
        m.tick(true, true, 0);
        let image = "https://icons.example.com/icons/image.png".to_owned();
        m.set_uploaded_icon(&"b".repeat(64), Some(image.clone()));
        m.tick(true, true, 100);
        assert_eq!(calls.lock().unwrap().sets.len(), 1);
        m.set_uploaded_icon(&"a".repeat(64), Some(image.clone()));
        m.tick(true, true, 200);
        {
            let sent = &calls.lock().unwrap().sets;
            assert_eq!(sent.len(), 2);
            assert_eq!(sent[1].large_image.as_ref(), Some(&image));
            assert_eq!(sent[0].application_name, sent[1].application_name);
            assert_eq!(sent[0].description, sent[1].description);
        }
        m.tick(true, true, 300);
        assert_eq!(calls.lock().unwrap().sets.len(), 2);
        m.set_enabled(false).unwrap();
        m.set_uploaded_icon(&"a".repeat(64), Some(image));
        assert!(m.uploaded_icon.is_none());
    }
}
