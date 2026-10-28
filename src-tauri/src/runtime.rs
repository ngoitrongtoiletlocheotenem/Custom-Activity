use crate::{
    commands,
    presence::service::Service,
    settings::{self, SettingsStore},
};
use std::{fs, io::Write};
use tauri::Manager;

/// Fixed technical event codes only. No paths, selected names, descriptions or SDK payloads.
pub fn record(event: &str) {
    let dir = settings::store::local_directory().join("logs");
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("currentactivity.log");
    if fs::metadata(&path)
        .map(|m| m.len() > 512 * 1024)
        .unwrap_or(false)
    {
        let _ = fs::rename(&path, dir.join("previous.log"));
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(file, "{time} {event}");
    }
}
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    #[cfg(debug_assertions)]
    if std::env::var_os("CURRENTACTIVITY_TEST_DATA_DIR").is_some() {
        // Native integration fixtures must not change the user's startup value.
        return if enabled {
            Err("Windows startup is disabled in the isolated test profile.".into())
        } else {
            Ok(())
        };
    }
    #[cfg(windows)]
    {
        use winreg::{enums::HKEY_CURRENT_USER, RegKey};
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
            .map_err(|_| "Cannot access your Windows startup settings.")?;
        if enabled {
            let exe =
                std::env::current_exe().map_err(|_| "Cannot locate CurrentActivity executable.")?;
            key.set_value(
                "CurrentActivity",
                &format!("\"{}\" --autostart", exe.display()),
            )
            .map_err(|_| "Cannot enable Windows startup.")?;
        } else if let Err(error) = key.delete_value("CurrentActivity") {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err("Cannot disable Windows startup.".into());
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err("Windows startup is available on Windows only.".into())
    }
}
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            crate::tray::open(app)
        }))
        .invoke_handler(tauri::generate_handler![
            commands::list_running_applications,
            commands::get_foreground_application,
            commands::select_application,
            commands::set_presence,
            commands::clear_presence,
            commands::set_sharing_enabled,
            commands::get_discord_connection_state,
            commands::get_activity_state,
            commands::get_settings,
            commands::get_icon_diagnostics,
            commands::update_settings,
            commands::configure_icon_upload,
            commands::complete_onboarding
        ])
        .setup(|app| {
            let store =
                SettingsStore::new(settings::store::config_directory().join("settings.json"));
            let (document, recovered) = store.load();
            if recovered {
                record("settings_recovered");
            }
            // Reconcile saved preference and current installed path, including after
            // corrupt-file recovery. Startup never restores sharing itself.
            if set_autostart(document.settings.start_with_windows).is_err() {
                record("startup_registration_failed");
            }
            let minimized =
                document.settings.start_minimized && document.settings.onboarding_complete;
            let cache = settings::store::local_directory().join("cache/icons");
            let service = Service::start(app.handle().clone(), document, store, cache);
            app.manage(service);
            crate::tray::setup(app.handle())?;
            if !minimized {
                crate::tray::open(app.handle());
            }
            record("started");
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("CurrentActivity could not start")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if let Some(service) = app.try_state::<Service>() {
                    if !service.is_exiting() {
                        api.prevent_exit();
                        service.send(crate::presence::service::Action::Exit);
                    }
                }
            }
        });
}
