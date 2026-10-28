use super::manager::{ActivityState, ConnectionState, Manager};
use crate::{
    applications::{self, DetectedApplication},
    discord::SocialSdkProvider,
    icon_upload::{credentials::TokenStore, IconUploadSettings, UploadCoordinator},
    settings::{Settings, SettingsDocument, SettingsStore},
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};

pub enum Action {
    List,
    Foreground,
    Select(String),
    SetPresence(String, String, Option<String>, Option<String>),
    Enable(bool),
    Toggle,
    Clear,
    State,
    Connection,
    Settings,
    #[cfg(debug_assertions)]
    IconDiagnostics,
    UpdateSettings(Settings),
    ConfigureIconUpload {
        enabled: bool,
        endpoint: String,
        public_base: String,
        token: Option<String>,
        consent: bool,
    },
    Onboarding,
    Exit,
}
pub enum Reply {
    Applications(Vec<DetectedApplication>),
    Application(Option<DetectedApplication>),
    State(ActivityState),
    Connection(ConnectionState),
    Settings(Settings),
    #[cfg(debug_assertions)]
    IconDiagnostics(serde_json::Value),
}
struct Request {
    action: Action,
    reply: Option<tokio::sync::oneshot::Sender<Result<Reply, String>>>,
}
#[derive(Clone)]
pub struct Service {
    sender: mpsc::Sender<Request>,
    exiting: Arc<AtomicBool>,
}
impl Service {
    pub fn is_exiting(&self) -> bool {
        self.exiting.load(Ordering::SeqCst)
    }
    pub async fn request(&self, action: Action) -> Result<Reply, String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.sender
            .send(Request {
                action,
                reply: Some(tx),
            })
            .map_err(|_| "Activity worker is unavailable.")?;
        rx.await
            .map_err(|_| "Activity worker stopped unexpectedly.".to_owned())?
    }
    pub fn send(&self, action: Action) {
        let _ = self.sender.send(Request {
            action,
            reply: None,
        });
    }
    pub fn start(
        app: AppHandle,
        document: SettingsDocument,
        store: SettingsStore,
        cache: PathBuf,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<Request>();
        let exiting = Arc::new(AtomicBool::new(false));
        let worker_exiting = exiting.clone();
        std::thread::Builder::new()
            .name("currentactivity-presence".into())
            .spawn(move || run_worker(app, document, store, cache, rx, worker_exiting))
            .expect("create presence worker");
        Self {
            sender: tx,
            exiting,
        }
    }
}

fn run_worker(
    app: AppHandle,
    document: SettingsDocument,
    store: SettingsStore,
    cache: PathBuf,
    rx: mpsc::Receiver<Request>,
    worker_exiting: Arc<AtomicBool>,
) {
    let client_id = document.settings.resolved_client_id();
    let mut manager = Manager::new(Box::new(SocialSdkProvider::new(client_id)), document);
    let token_store = TokenStore::new(store.path.with_file_name("icon-upload-token.dpapi"));
    manager.document.settings.icon_upload.credential_stored =
        token_store.credential_stored(&manager.document.settings.icon_upload.endpoint);
    let mut uploads =
        UploadCoordinator::new(cache.parent().unwrap_or(&cache).join("uploaded-icons.json"));
    let mut known = HashMap::<String, DetectedApplication>::new();
    let mut foreground: Option<DetectedApplication> = None;
    let mut running = false;
    let mut discord = false;
    let start = Instant::now();
    let mut monitored = Instant::now() - Duration::from_secs(2);
    let mut foreground_checked = Instant::now() - Duration::from_secs(1);
    let mut previous = manager.state.clone();
    loop {
        if monitored.elapsed() >= Duration::from_millis(1500) {
            discord = applications::is_discord_running();
            running = manager
                .state
                .selected_application
                .as_ref()
                .is_some_and(|a| applications::is_application_running(&a.executable_path));
            monitored = Instant::now();
        }
        // Remember only executable metadata before our own picker takes focus.
        if foreground_checked.elapsed() >= Duration::from_millis(500) {
            if let Ok(Some(application)) = applications::get_foreground_application(&cache) {
                if foreground.as_ref().map(|a| &a.id) != Some(&application.id) {
                    known.insert(application.id.clone(), application.clone());
                    foreground = Some(application);
                }
            }
            foreground_checked = Instant::now();
        }
        let mut exit = false;
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(request) => {
                let result = (|| -> Result<Reply, String> {
                    match request.action {
                        Action::List => {
                            let apps = applications::list_running_applications(&cache)?;
                            for app in &apps {
                                known.insert(app.id.clone(), app.clone());
                            }
                            Ok(Reply::Applications(apps))
                        }
                        Action::Foreground => {
                            if let Ok(Some(a)) = applications::get_foreground_application(&cache) {
                                foreground = Some(a);
                            }
                            let app = foreground.clone().filter(|a| {
                                applications::is_application_running(&a.executable_path)
                            });
                            if let Some(a) = &app {
                                known.insert(a.id.clone(), a.clone());
                            }
                            Ok(Reply::Application(app))
                        }
                        Action::State => Ok(Reply::State(manager.state.clone())),
                        Action::Connection => {
                            Ok(Reply::Connection(manager.state.discord_state.clone()))
                        }
                        Action::Settings => Ok(Reply::Settings(manager.document.settings.clone())),
                        #[cfg(debug_assertions)]
                        Action::IconDiagnostics => {
                            Ok(Reply::IconDiagnostics(manager.icon_diagnostics(&cache)))
                        }
                        Action::Select(id) => {
                            let mut app = known
                                .get(&id)
                                .cloned()
                                .ok_or("Refresh the application list and select an application.")?;
                            app.running =
                                applications::is_application_running(&app.executable_path);
                            if !app.running {
                                return Err("Selected application is no longer running.".into());
                            }
                            manager.select(app)?;
                            running = true;
                            discord = applications::is_discord_running();
                            manager.tick(discord, running, start.elapsed().as_millis() as u64);
                            Ok(Reply::State(manager.state.clone()))
                        }
                        Action::SetPresence(id, text, application_name_override, details) => {
                            running =
                                manager
                                    .state
                                    .selected_application
                                    .as_ref()
                                    .is_some_and(|a| {
                                        applications::is_application_running(&a.executable_path)
                                    });
                            discord = applications::is_discord_running();
                            manager.tick(discord, running, start.elapsed().as_millis() as u64);
                            let name = application_name_override
                                .unwrap_or_else(|| manager.state.application_name_override.clone());
                            let details = details.unwrap_or_else(|| manager.state.details.clone());
                            manager.submit_presence_with_overrides(
                                &id,
                                text,
                                name,
                                details,
                                |document| store.save(document),
                            )?;
                            manager.tick(discord, running, start.elapsed().as_millis() as u64);
                            Ok(Reply::State(manager.state.clone()))
                        }
                        Action::Enable(value) => {
                            running =
                                manager
                                    .state
                                    .selected_application
                                    .as_ref()
                                    .is_some_and(|a| {
                                        applications::is_application_running(&a.executable_path)
                                    });
                            // Observe a process stop before an enable request can fail.
                            // Assigning the flag directly would hide the transition
                            // from tick and leave the old activity/session published.
                            discord = applications::is_discord_running();
                            manager.tick(discord, running, start.elapsed().as_millis() as u64);
                            manager.set_enabled(value)?;
                            manager.tick(discord, running, start.elapsed().as_millis() as u64);
                            Ok(Reply::State(manager.state.clone()))
                        }
                        Action::Toggle => {
                            let value = !manager.state.enabled;
                            running =
                                manager
                                    .state
                                    .selected_application
                                    .as_ref()
                                    .is_some_and(|a| {
                                        applications::is_application_running(&a.executable_path)
                                    });
                            discord = applications::is_discord_running();
                            manager.tick(discord, running, start.elapsed().as_millis() as u64);
                            manager.set_enabled(value)?;
                            manager.tick(discord, running, start.elapsed().as_millis() as u64);
                            Ok(Reply::State(manager.state.clone()))
                        }
                        Action::Clear => {
                            manager.set_enabled(false)?;
                            Ok(Reply::State(manager.state.clone()))
                        }
                        Action::UpdateSettings(mut settings) => {
                            // Only ConfigureIconUpload can change consent, destination or
                            // credential state. General preference saves may be stale.
                            settings.icon_upload = manager.document.settings.icon_upload.clone();
                            settings.validate()?;
                            settings.onboarding_complete =
                                manager.document.settings.onboarding_complete;
                            let before = manager.document.clone();
                            if before.settings.start_with_windows != settings.start_with_windows {
                                crate::runtime::set_autostart(settings.start_with_windows)?;
                            }
                            let id_changed =
                                before.settings.discord_client_id != settings.discord_client_id;
                            manager.update_settings(settings.clone())?;
                            if let Err(error) = store.save(&manager.document) {
                                let _ = crate::runtime::set_autostart(
                                    before.settings.start_with_windows,
                                );
                                manager.document = before;
                                return Err(error);
                            }
                            if id_changed {
                                manager.replace_provider(Box::new(SocialSdkProvider::new(
                                    settings.resolved_client_id(),
                                )));
                            }
                            let _ = app.emit("settings-changed", &settings);
                            Ok(Reply::Settings(settings))
                        }
                        Action::ConfigureIconUpload {
                            enabled,
                            endpoint,
                            public_base,
                            token,
                            consent,
                        } => {
                            if enabled && !consent {
                                return Err("Explicit permission to upload the selected executable icon is required.".into());
                            }
                            let mut config = IconUploadSettings {
                                enabled,
                                consent_granted: enabled && consent,
                                endpoint,
                                public_base,
                                credential_stored: false,
                            }
                            .normalized();
                            config.validate()?;
                            let token = token.filter(|value| !value.is_empty());
                            let endpoint_changed =
                                config.endpoint != manager.document.settings.icon_upload.endpoint;
                            if enabled && endpoint_changed && token.is_none() {
                                return Err(
                                    "Supply a new Worker token when changing the upload endpoint."
                                        .into(),
                                );
                            }
                            let old_credential = token_store.snapshot();
                            if let Some(token) = token {
                                crate::icon_upload::validate_base(&config.endpoint)?;
                                token_store.save(&config.endpoint, &token)?;
                            }
                            config.credential_stored =
                                token_store.credential_stored(&config.endpoint);
                            if enabled && !config.credential_stored {
                                token_store.restore(old_credential);
                                return Err(
                                    "Configure a Worker token before enabling icon upload.".into(),
                                );
                            }
                            let before = manager.document.clone();
                            manager.document.settings.icon_upload = config;
                            if let Err(error) = store.save(&manager.document) {
                                manager.document = before;
                                token_store.restore(old_credential);
                                return Err(error);
                            }
                            // Clearing an old override lets mapped/fallback presence remain
                            // immediate while a new destination's upload is in flight.
                            manager.set_uploaded_icon("", None);
                            uploads.reset();
                            let settings = manager.document.settings.clone();
                            let _ = app.emit("settings-changed", &settings);
                            Ok(Reply::Settings(settings))
                        }
                        Action::Onboarding => {
                            let mut doc = manager.document.clone();
                            doc.settings.onboarding_complete = true;
                            store.save(&doc)?;
                            manager.document = doc;
                            let settings = manager.document.settings.clone();
                            let _ = app.emit("settings-changed", &settings);
                            Ok(Reply::Settings(settings))
                        }
                        Action::Exit => {
                            worker_exiting.store(true, Ordering::SeqCst);
                            manager.shutdown();
                            exit = true;
                            Ok(Reply::State(manager.state.clone()))
                        }
                    }
                })();
                if result.is_err() {
                    crate::runtime::record("command_failed");
                }
                if let Some(reply) = request.reply {
                    let _ = reply.send(result);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                manager.shutdown();
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let uploaded = uploads.step(
            &manager.document.settings.icon_upload,
            manager.state.selected_application.as_ref(),
            manager.state.enabled && running && !manager.state.resume_required && !exit,
            &token_store,
        );
        let selected_id = manager
            .state
            .selected_application
            .as_ref()
            .map(|app| app.id.clone())
            .unwrap_or_default();
        manager.set_uploaded_icon(&selected_id, uploaded);
        manager.tick(discord, running, start.elapsed().as_millis() as u64);
        if previous != manager.state {
            if previous.discord_state != manager.state.discord_state {
                let _ = app.emit("discord-state-changed", &manager.state.discord_state);
                if manager.state.discord_state.status == "error" {
                    crate::runtime::record("discord_connection_failed");
                }
            }
            if previous.selected_application_running && !manager.state.selected_application_running
            {
                let _ = app.emit("application-stopped", &manager.state);
            }
            let _ = app.emit("presence-updated", &manager.state);
            crate::tray::update(&app, &manager.state);
            previous = manager.state.clone();
        }
        if exit {
            app.exit(0);
            break;
        }
    }
}
