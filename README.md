# Custom Activity

A Windows desktop app for sharing one running application on Discord. Choose a program, customize its displayed name, activity title and details, and enable Rich Presence. It extracts executable icons locally, can upload the selected icon through an optional Cloudflare R2 host, and keeps elapsed activity time across Discord reconnects. Sharing starts off at every launch.

## Features

- **Application picker**: search running desktop programs or select the active application. Multiple windows from the same executable appear as one entry.
- **Custom presence**: override the application name and edit two lines of activity text. Drafts stay local until **Update Presence**, **Ctrl+Enter**, or enabling sharing. Submitted text can be remembered per application.
- **Executable icons**: extract icons through Windows Shell APIs and embedded PE resources, preserve transparency, and cache results. Extraction never starts the target executable.
- **Elapsed time**: keep the same session start through Discord/network reconnects and text or icon updates. Disabling sharing, changing the selected executable, detecting its stop, or restarting the app clears it; the next sharing session starts fresh.
- **Optional icon hosting**: upload the selected PNG through an authenticated Cloudflare Worker to R2, then use its public URL on Discord. Uploads require separate consent and active sharing; the upload token is protected with Windows DPAPI.
- **Tray and preferences**: close to the system tray, reopen the existing instance, opt into Windows startup or minimized launch, and configure reconnect and application-resume behavior.
- **Privacy controls**: share only the selected application's name or override, submitted text, session timestamp and image reference. No window titles, document contents, browser history or process memory are collected. No application telemetry.

## Architecture

```text
React + TypeScript (WebView2)
          │
          │ Tauri commands / events
          ▼
     Rust backend ──┬──► Windows APIs (programs, icons, tray, settings)
                    ├──► C++ bridge ──► Discord Social SDK ──► Discord Desktop
                    └──► HTTPS upload (opt-in) ──► Cloudflare Worker ──► R2
```

The backend validates the hosted image URL before passing it to Discord. The real local icon preview works independently of the image displayed by Discord.

| **Part** | **Stack** |
| --- | --- |
| Interface | React 19, TypeScript 5.9, Vite 8, Lucide icons, CSS |
| Desktop runtime | Tauri 2, Microsoft Edge WebView2 |
| Native backend | Rust, Windows APIs, local JSON settings, DPAPI credentials |
| Discord integration | Official Discord Social SDK for Windows x64 through a C++20 bridge |
| Optional hosting | TypeScript Cloudflare Worker, R2, SHA-256 content-addressed PNGs |
| Tests | Vitest, React Testing Library, Rust tests, Node.js Worker tests |


## Quick start (Windows)

Requires Windows 10/11 x64, Node.js 24 LTS, Rust with the MSVC target, Visual Studio C++ Build Tools with a Windows SDK, and Edge WebView2. The Rust toolchain must meet the minimum in [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml), currently 1.99.

For Discord publishing, also use a signed-in Discord Desktop client and obtain the official Windows x64 Social SDK through your [Discord Developer Application](https://discord.com/developers/applications). Review the SDK's terms and eligibility requirements, then extract it outside this repository. The inner SDK directory must contain:

```text
include/discordpp.h
lib/release/discord_partner_sdk.lib
bin/release/discord_partner_sdk.dll
License-Notices.txt
```

From a Developer PowerShell at the repository root:

```powershell
npm ci
$env:DISCORD_SOCIAL_SDK_DIR = 'C:\SDKs\discord_social_sdk'
npm run tauri -- dev
```

Replace the SDK path with your extracted directory. In the app, complete the privacy introduction and enter your public numeric Discord Application ID in **Settings**. Enable activity display in Discord's privacy settings, select a running program, enter any activity text, and turn on sharing.

The Application ID is public. Discord publishing uses the SDK's token-free desktop flow; no Discord client secret, bot token or user token is needed. 

### Browser preview and SDK-free development

```powershell
npm run dev
```

Open [http://127.0.0.1:1420](http://127.0.0.1:1420) for the browser preview. Native application detection and Discord connectivity are available only in the desktop app.

You can also run the desktop app without the SDK:

```powershell
Remove-Item Env:DISCORD_SOCIAL_SDK_DIR -ErrorAction SilentlyContinue
npm run tauri -- dev
```

Local detection, icons, preferences and tray behavior remain available. Publishing reports an SDK configuration error until you build with the official SDK.

## Build for Windows

In a Developer PowerShell with the SDK directory configured, supply the public Application ID to embed in the build:

```powershell
$env:DISCORD_CLIENT_ID = 'YOUR_NUMERIC_APPLICATION_ID'
./scripts/package.ps1
```

Replace the placeholder with a positive numeric ID before running the command. You can instead set `discordClientId` in [`config/app.json`](config/app.json).

| **Output** | **Contents** |
| --- | --- |
| `src-tauri/target/release/CurrentActivity.exe` | Desktop executable; keep the SDK DLL beside it when running it separately |
| `src-tauri/target/release/bundle/nsis/` | Per-user Windows installer, including the SDK runtime and bundled notices |

For an SDK-free preview installer:

```powershell
Remove-Item Env:DISCORD_SOCIAL_SDK_DIR -ErrorAction SilentlyContinue
./scripts/package.ps1 -Preview
```

Preview installers cannot publish Discord activity. The installer embeds the WebView2 bootstrapper, which may download Microsoft's runtime. Review SDK redistribution terms, configure code signing and verify installation on a clean Windows profile before distributing a build.

## Optional Cloudflare R2 hosting

Executable icons stay local while uploads are off. Discord uses a configured asset key, public image mapping or generic fallback. To display the selected executable's extracted icon on Discord, deploy the optional Worker against an existing R2 bucket you control.

From `cloudflare/`:

```powershell
npm ci --ignore-scripts
./Prepare-Configuration.ps1 -BucketName 'your-existing-bucket'
$env:WRANGLER_SEND_METRICS = 'false'
npx wrangler login
npx wrangler secret put ICON_UPLOAD_TOKEN
npm run deploy
```

At the secret prompt, enter a random upload token of 32–1,024 printable ASCII characters without spaces. Configure the Worker origin, public image base and matching token in the desktop app's upload settings, then explicitly enable icon uploads and consent.

By default, the Worker serves public images through its R2 binding at its own origin. An existing public image domain is also supported. A public R2 read address alone does not authorize uploads. Keep Cloudflare API credentials and R2 S3 access keys out of the desktop app and source code.

Uploads contain only the selected PNG bytes and bearer authentication. Images use SHA-256 object keys and are limited to 256 × 256 pixels and 128 KiB. Hosted images remain public until the bucket operator removes them.


## Testing

From the repository root, after installing dependencies:

```powershell
# Frontend: type checking, production build and component tests
npm run build
npm test

# Rust: formatting, linting and unit/lifecycle tests
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml

# Optional Worker: type checking and in-memory R2 tests
npm ci --prefix cloudflare
npm --prefix cloudflare run check
npm --prefix cloudflare test
```

Rust lifecycle tests inject provider, process and timing state. Frontend tests mock the desktop commands/events. Worker tests use in-memory storage and do not deploy or access a real bucket.

[GitHub Actions](.github/workflows/ci.yml) runs these checks on pushes and pull requests, builds an SDK-free desktop preview, and uploads its executable. Real Discord publication requires your separately installed SDK and configured Application ID.

To inspect an executable's icon without starting it:

```powershell
cargo run --manifest-path src-tauri/Cargo.toml --no-default-features --example test_icon -- 'C:\Program Files\Example\Example.exe'
```


## Configuration

Build environment variables are documented in [`.env.example`](.env.example). The file is not loaded automatically; set variables in your PowerShell session or configure the public project defaults directly.

| **Setting** | **Purpose** |
| --- | --- |
| `DISCORD_SOCIAL_SDK_DIR` | Extracted official SDK root containing headers, import library, runtime DLL and notices |
| `DISCORD_CLIENT_ID` | Public numeric Application ID; overrides the project default at build time |
| `config/app.json` → `discordClientId` | Compiled default Application ID; blank in this source tree |
| Settings → Discord Application ID | Per-user ID; overrides the compiled default |
| `config/app.json` → `iconUploadPublicBase` | Optional compiled public image base; blank by default and separate from the upload endpoint |
| Settings → icon uploads | Worker origin, public image base, upload token and explicit consent; uploads default off |
| Settings → Remember descriptions | Remember submitted titles, name overrides and details per application on this device |
| Settings → startup, reconnect and auto-resume | Control Windows startup, minimized launch, connection recovery and resuming a restarted program |


## Limitations

The picker includes eligible visible desktop windows. Background services, hosted UWP identities and inaccessible processes may be unavailable. Process checks and in-flight SDK calls make clearing best effort, and Discord's own activity detection can remain visible independently. Closing the selected application clears its session; automatic resumption is off by default.

## Project layout

```text
.
├── src/                       # React interface, command client and frontend tests
├── src-tauri/
│   ├── src/applications/      # Windows discovery, icon extraction and image mapping
│   ├── src/discord/           # Validated presence model and SDK provider
│   ├── src/presence/          # Sharing lifecycle and background service
│   ├── src/icon_upload/       # Upload consent, HTTPS transport, cache and DPAPI
│   ├── src/settings/          # Local preferences and remembered presentation text
│   ├── src/tray/              # Tray controls
│   ├── icons/                # Windows application and installer icons
│   └── tauri.conf.json        # Desktop window, content security policy and packaging
├── native/                    # C++ bridge to the official Discord Social SDK
├── cloudflare/                # Optional TypeScript Worker, R2 configuration and tests
├── assets/                    # Source artwork, Discord image mappings and notices
├── config/app.json            # Blank public configuration defaults
├── scripts/                   # Packaging, icon generation and native verification
├── docs/                      # SDK setup and icon pipeline documentation
└── .github/workflows/ci.yml    # Windows build and test checks
```
