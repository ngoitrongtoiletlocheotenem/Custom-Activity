use std::{env, path::PathBuf};
fn main() {
    println!("cargo:rustc-check-cfg=cfg(discord_social_sdk)");
    println!("cargo:rerun-if-env-changed=DISCORD_SOCIAL_SDK_DIR");
    println!("cargo:rerun-if-env-changed=DISCORD_CLIENT_ID");
    println!("cargo:rerun-if-env-changed=CURRENTACTIVITY_REQUIRE_SDK");
    println!("cargo:rerun-if-changed=../native/social_sdk_bridge.cpp");
    println!("cargo:rerun-if-changed=../config/app.json");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/icon.png");
    let config: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string("../config/app.json").expect("read config/app.json"),
    )
    .expect("valid config/app.json");
    let base = config["iconUploadPublicBase"].as_str().unwrap_or("");
    if !base.is_empty() {
        assert!(
            base.starts_with("https://") && !base.chars().any(char::is_control),
            "Icon upload public base must be HTTPS"
        );
    }
    println!("cargo:rustc-env=CURRENTACTIVITY_ICON_PUBLIC_BASE={base}");
    let client_id = env::var("DISCORD_CLIENT_ID")
        .ok()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| config["discordClientId"].as_str().unwrap_or("").to_owned());
    if !client_id.is_empty() {
        assert!(
            client_id.parse::<u64>().is_ok_and(|id| id != 0),
            "Discord Application ID must be a positive numeric identifier"
        );
        println!("cargo:rustc-env=DISCORD_CLIENT_ID={client_id}");
    }
    if let Ok(sdk) = env::var("DISCORD_SOCIAL_SDK_DIR") {
        let sdk = PathBuf::from(sdk);
        let include = sdk.join("include");
        let lib = sdk.join("lib/release");
        let dll = sdk.join("bin/release/discord_partner_sdk.dll");
        assert!(
            include.join("discordpp.h").is_file(),
            "SDK include/discordpp.h is missing"
        );
        assert!(
            lib.join("discord_partner_sdk.lib").is_file(),
            "SDK lib/release/discord_partner_sdk.lib is missing"
        );
        assert!(
            dll.is_file(),
            "SDK bin/release/discord_partner_sdk.dll is missing"
        );
        let mut bridge = cc::Build::new();
        if env::var("DEBUG").as_deref() == Ok("true") {
            bridge.define("CA_DISCORD_DIAGNOSTICS", None);
        }
        bridge
            .cpp(true)
            .static_crt(true)
            .std("c++20")
            .flag_if_supported("/EHsc")
            .include(include)
            .file("../native/social_sdk_bridge.cpp")
            .compile("currentactivity_discord_bridge");
        println!("cargo:rustc-link-search=native={}", lib.display());
        println!("cargo:rustc-link-lib=dylib=discord_partner_sdk");
        println!("cargo:rustc-link-arg=/DELAYLOAD:discord_partner_sdk.dll");
        println!("cargo:rustc-link-lib=delayimp");
        println!("cargo:rustc-cfg=discord_social_sdk");
        let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
        let profile = out.ancestors().nth(3).unwrap();
        let runtime = profile.join("discord_partner_sdk.dll");
        // Do not overwrite an identical DLL locked by a running development app.
        let bytes = std::fs::read(&dll).expect("read SDK runtime");
        if std::fs::read(&runtime).ok().as_deref() != Some(bytes.as_slice()) {
            std::fs::write(runtime, bytes)
                .expect("copy SDK runtime; close CurrentActivity when changing SDK versions");
        }
    } else if env::var("CURRENTACTIVITY_REQUIRE_SDK").as_deref() == Ok("1") {
        panic!(
            "Production presence build requires DISCORD_SOCIAL_SDK_DIR. See docs/DISCORD_SDK.md"
        );
    } else {
        println!("cargo:warning=Official Discord SDK not supplied: presence reports a configuration error; no substitute protocol is used.");
    }
    if env::var_os("CARGO_FEATURE_DESKTOP").is_some() {
        tauri_build::build();
    }
}
