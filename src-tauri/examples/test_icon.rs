//! Explicit local development diagnostics. The target executable is never started.
#[cfg(debug_assertions)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use currentactivity_lib::applications::icon_extractor::diagnose_application_icon;
    let executable = std::env::args_os()
        .nth(1)
        .ok_or("Usage: cargo run --example test_icon -- <executable path>")?;
    let executable = std::path::Path::new(&executable);
    println!("[IconExtractor]\nExecutable: {}", executable.display());
    let icon = diagnose_application_icon(executable)?;
    for attempt in &icon.attempts {
        println!("Attempt: {attempt}");
    }
    let directory = std::env::temp_dir().join(format!(
        "CurrentActivity-icon-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory)?;
    let original = directory.join("original.png");
    let upload = directory.join("upload.png");
    std::fs::write(&original, &icon.original_png)?;
    std::fs::write(&upload, &icon.png)?;
    println!("Selected source: {}\nOriginal: {}x{} RGBA PNG\nOriginal PNG: {}\nHosting PNG: {} ({} bytes)",icon.source,icon.width,icon.height,original.display(),upload.display(),icon.png.len());
    Ok(())
}
#[cfg(not(debug_assertions))]
fn main() {
    eprintln!("Icon diagnostics are available in development builds only.");
}
