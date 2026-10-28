/// Read-only native smoke check. Prints friendly names and filenames only, never paths/titles.
fn main() {
    let cache = currentactivity_lib::settings::store::local_directory().join("cache/icons");
    let apps = currentactivity_lib::applications::list_running_applications(&cache)
        .expect("Windows application enumeration");
    let mut identities = std::collections::HashSet::new();
    for app in &apps {
        assert!(identities.insert(&app.id), "duplicate executable identity");
        println!(
            "{} ({}) — icon {}, running {}",
            app.name,
            app.executable_name,
            app.icon_data_url.is_some(),
            currentactivity_lib::applications::is_application_running(&app.executable_path)
        );
    }
    println!(
        "{} applications; Discord Desktop detected: {}",
        apps.len(),
        currentactivity_lib::applications::is_discord_running()
    );
}
