use crate::presence::{
    manager::ActivityState,
    service::{Action, Service},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};
pub struct Items {
    sharing: MenuItem<tauri::Wry>,
    current: MenuItem<tauri::Wry>,
}
pub fn open(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let title = MenuItem::with_id(app, "title", "CurrentActivity", false, None::<&str>)?;
    let open_item = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
    let sharing = MenuItem::with_id(app, "sharing", "Sharing: Off", true, None::<&str>)?;
    let current = MenuItem::with_id(app, "current", "Current: None", false, None::<&str>)?;
    let clear = MenuItem::with_id(app, "clear", "Clear Presence", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "Exit", true, None::<&str>)?;
    let line1 = tauri::menu::PredefinedMenuItem::separator(app)?;
    let line2 = tauri::menu::PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &title, &line1, &open_item, &sharing, &current, &line2, &clear, &exit,
        ],
    )?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("application icon".into()))?;
    TrayIconBuilder::with_id("currentactivity")
        .icon(icon)
        .tooltip("CurrentActivity — activity disabled")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => open(app),
            "sharing" => app.state::<Service>().send(Action::Toggle),
            "clear" => app.state::<Service>().send(Action::Clear),
            "exit" => app.state::<Service>().send(Action::Exit),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                open(tray.app_handle())
            } else if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            { /* Double-click opens; single click leaves the user's focus alone. */
            }
        })
        .build(app)?;
    app.manage(Items { sharing, current });
    Ok(())
}
pub fn update(app: &AppHandle, state: &ActivityState) {
    if let Some(items) = app.try_state::<Items>() {
        let _ = items.sharing.set_text(if state.enabled {
            "Sharing: On"
        } else {
            "Sharing: Off"
        });
        let name = state
            .selected_application
            .as_ref()
            .map(|a| a.name.as_str())
            .unwrap_or("None");
        let _ = items.current.set_text(format!("Current: {name}"));
    }
}
