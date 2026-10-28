use crate::{
    applications::DetectedApplication,
    presence::{
        manager::{ActivityState, ConnectionState},
        service::{Action, Reply, Service},
    },
    settings::Settings,
};
use tauri::State;
fn wrong() -> String {
    "Unexpected response from activity worker.".into()
}
#[tauri::command]
pub async fn get_icon_diagnostics(
    service: State<'_, Service>,
) -> Result<serde_json::Value, String> {
    #[cfg(debug_assertions)]
    {
        match service.request(Action::IconDiagnostics).await? {
            Reply::IconDiagnostics(value) => Ok(value),
            _ => Err(wrong()),
        }
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = service;
        Err("Icon diagnostics are available in development builds only.".into())
    }
}
#[tauri::command]
pub async fn list_running_applications(
    service: State<'_, Service>,
) -> Result<Vec<DetectedApplication>, String> {
    match service.request(Action::List).await? {
        Reply::Applications(v) => Ok(v),
        _ => Err(wrong()),
    }
}
#[tauri::command]
pub async fn get_foreground_application(
    service: State<'_, Service>,
) -> Result<Option<DetectedApplication>, String> {
    match service.request(Action::Foreground).await? {
        Reply::Application(v) => Ok(v),
        _ => Err(wrong()),
    }
}
#[tauri::command]
pub async fn select_application(
    service: State<'_, Service>,
    app_id: String,
) -> Result<ActivityState, String> {
    match service.request(Action::Select(app_id)).await? {
        Reply::State(v) => Ok(v),
        _ => Err(wrong()),
    }
}
#[tauri::command]
pub async fn set_presence(
    service: State<'_, Service>,
    app_id: String,
    description: String,
    application_name_override: Option<String>,
    details: Option<String>,
) -> Result<ActivityState, String> {
    match service
        .request(Action::SetPresence(
            app_id,
            description,
            application_name_override,
            details,
        ))
        .await?
    {
        Reply::State(v) => Ok(v),
        _ => Err(wrong()),
    }
}
#[tauri::command]
pub async fn clear_presence(service: State<'_, Service>) -> Result<ActivityState, String> {
    match service.request(Action::Clear).await? {
        Reply::State(v) => Ok(v),
        _ => Err(wrong()),
    }
}
#[tauri::command]
pub async fn set_sharing_enabled(
    service: State<'_, Service>,
    enabled: bool,
) -> Result<ActivityState, String> {
    match service.request(Action::Enable(enabled)).await? {
        Reply::State(v) => Ok(v),
        _ => Err(wrong()),
    }
}
#[tauri::command]
pub async fn get_activity_state(service: State<'_, Service>) -> Result<ActivityState, String> {
    match service.request(Action::State).await? {
        Reply::State(v) => Ok(v),
        _ => Err(wrong()),
    }
}
#[tauri::command]
pub async fn get_discord_connection_state(
    service: State<'_, Service>,
) -> Result<ConnectionState, String> {
    match service.request(Action::Connection).await? {
        Reply::Connection(v) => Ok(v),
        _ => Err(wrong()),
    }
}
#[tauri::command]
pub async fn get_settings(service: State<'_, Service>) -> Result<Settings, String> {
    match service.request(Action::Settings).await? {
        Reply::Settings(v) => Ok(v),
        _ => Err(wrong()),
    }
}
#[tauri::command]
pub async fn update_settings(
    service: State<'_, Service>,
    settings: Settings,
) -> Result<Settings, String> {
    match service.request(Action::UpdateSettings(settings)).await? {
        Reply::Settings(v) => Ok(v),
        _ => Err(wrong()),
    }
}
#[tauri::command]
pub async fn configure_icon_upload(
    service: State<'_, Service>,
    enabled: bool,
    endpoint: String,
    public_base: String,
    token: Option<String>,
    consent: bool,
) -> Result<Settings, String> {
    match service
        .request(Action::ConfigureIconUpload {
            enabled,
            endpoint,
            public_base,
            token,
            consent,
        })
        .await?
    {
        Reply::Settings(value) => Ok(value),
        _ => Err(wrong()),
    }
}
#[tauri::command]
pub async fn complete_onboarding(service: State<'_, Service>) -> Result<Settings, String> {
    match service.request(Action::Onboarding).await? {
        Reply::Settings(v) => Ok(v),
        _ => Err(wrong()),
    }
}
