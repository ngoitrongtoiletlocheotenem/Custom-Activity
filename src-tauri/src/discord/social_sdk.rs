use super::provider::{validate_activity, PresenceActivity, PresenceProvider};

pub struct SocialSdkProvider {
    client_id: Option<String>,
    #[cfg(discord_social_sdk)]
    handle: Option<std::ptr::NonNull<std::ffi::c_void>>,
    #[cfg(debug_assertions)]
    last_diagnostics: Option<String>,
}

// The provider is exclusively owned by the presence worker. Every SDK call,
// including callbacks and destruction, is serialized through &mut self.
#[cfg(discord_social_sdk)]
unsafe impl Send for SocialSdkProvider {}

impl SocialSdkProvider {
    pub fn new(client_id: Option<String>) -> Self {
        Self {
            client_id,
            #[cfg(discord_social_sdk)]
            handle: None,
            #[cfg(debug_assertions)]
            last_diagnostics: None,
        }
    }
    fn application_id(&self) -> Result<u64, String> {
        self.client_id
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|id| *id != 0)
            .ok_or_else(|| "Configure a valid Discord Application ID before sharing.".into())
    }
    #[cfg(all(discord_social_sdk, target_os = "windows"))]
    fn ensure_runtime() -> Result<(), String> {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn LoadLibraryExW(
                path: *const u16,
                file: *mut std::ffi::c_void,
                flags: u32,
            ) -> *mut std::ffi::c_void;
        }
        // /DELAYLOAD in build.rs keeps a missing SDK from preventing app startup.
        // Resolve only the runtime next to this executable, never a CWD DLL.
        static RUNTIME: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
        RUNTIME.get_or_init(|| {
            let path = std::env::current_exe().map_err(|_| "Cannot locate the application executable.".to_string())?
                .with_file_name("discord_partner_sdk.dll");
            if !path.is_file() { return Err("Discord Social SDK runtime is missing beside CurrentActivity. Reinstall an SDK-enabled build.".into()); }
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let module = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), 0x100 | 0x1000) };
            if module.is_null() { return Err("Discord Social SDK runtime could not load. Install the Windows x64 runtime dependencies.".into()); }
            // Keep loaded for process lifetime: SDK globals can outlive a provider.
            Ok(())
        }).clone()
    }
}

#[cfg(discord_social_sdk)]
mod ffi {
    use std::ffi::{c_char, c_void};
    extern "C" {
        pub fn ca_discord_create(id: u64) -> *mut c_void;
        pub fn ca_discord_destroy(handle: *mut c_void);
        pub fn ca_discord_connect(handle: *mut c_void) -> i32;
        pub fn ca_discord_connected(handle: *mut c_void) -> i32;
        pub fn ca_discord_set(
            handle: *mut c_void,
            name: *const c_char,
            details: *const c_char,
            image: *const c_char,
            image_text: *const c_char,
            state: *const c_char,
            started_at_seconds: u64,
        ) -> i32;
        pub fn ca_discord_clear(handle: *mut c_void) -> i32;
        pub fn ca_discord_tick(handle: *mut c_void) -> i32;
        #[cfg(debug_assertions)]
        pub fn ca_discord_diagnostics(
            handle: *mut c_void,
            output: *mut c_char,
            capacity: usize,
        ) -> usize;
    }
    pub fn result(code: i32) -> Result<(), String> {
        match code {
            0 => Ok(()),
            2 => Err(
                "Discord did not respond. Check that Discord Desktop is running and retry.".into(),
            ),
            _ => {
                Err("Discord rejected the request or its desktop connection is unavailable.".into())
            }
        }
    }
}

impl PresenceProvider for SocialSdkProvider {
    fn connect(&mut self) -> Result<(), String> {
        let id = self.application_id()?;
        #[cfg(discord_social_sdk)]
        {
            #[cfg(target_os = "windows")]
            Self::ensure_runtime()?;
            if self.handle.is_none() {
                self.handle = std::ptr::NonNull::new(unsafe { ffi::ca_discord_create(id) });
            }
            let handle = self
                .handle
                .ok_or("Discord Social SDK initialization failed.")?;
            ffi::result(unsafe { ffi::ca_discord_connect(handle.as_ptr()) })
        }
        #[cfg(not(discord_social_sdk))]
        {
            let _ = id;
            Err("This build does not include the official Discord Social SDK. See docs/DISCORD_SDK.md.".into())
        }
    }
    fn disconnect(&mut self) -> Result<(), String> {
        #[cfg(discord_social_sdk)]
        if let Some(handle) = self.handle.take() {
            unsafe {
                ffi::ca_discord_clear(handle.as_ptr());
                ffi::ca_discord_destroy(handle.as_ptr());
            }
        }
        Ok(())
    }
    fn set_activity(&mut self, activity: &PresenceActivity) -> Result<(), String> {
        validate_activity(activity)?;
        #[cfg(discord_social_sdk)]
        {
            use std::ffi::CString;
            let handle = self.handle.ok_or("Discord is not connected.")?;
            let fields = [
                &activity.application_name,
                activity.description.as_deref().unwrap_or(""),
                activity.large_image.as_deref().unwrap_or(""),
                activity.large_image_text.as_deref().unwrap_or(""),
                activity.details.as_deref().unwrap_or(""),
            ]
            .map(|v| {
                CString::new(v).map_err(|_| "Activity contains an invalid character.".to_string())
            });
            let [name, details, image, text, state] = fields;
            let (name, details, image, text, state) = (name?, details?, image?, text?, state?);
            let result = ffi::result(unsafe {
                ffi::ca_discord_set(
                    handle.as_ptr(),
                    name.as_ptr(),
                    details.as_ptr(),
                    image.as_ptr(),
                    text.as_ptr(),
                    state.as_ptr(),
                    activity.started_at.unwrap_or(0),
                )
            });
            #[cfg(debug_assertions)]
            if let Some(diagnostics) = self.diagnostics_json() {
                eprintln!("[CurrentActivity icon SDK diagnostics] {diagnostics}");
                self.last_diagnostics = Some(diagnostics);
            }
            result
        }
        #[cfg(not(discord_social_sdk))]
        {
            self.connect()
        }
    }
    fn clear_activity(&mut self) -> Result<(), String> {
        #[cfg(discord_social_sdk)]
        if let Some(handle) = self.handle {
            return ffi::result(unsafe { ffi::ca_discord_clear(handle.as_ptr()) });
        }
        Ok(())
    }
    fn is_connected(&self) -> bool {
        #[cfg(discord_social_sdk)]
        {
            self.handle
                .is_some_and(|h| unsafe { ffi::ca_discord_connected(h.as_ptr()) != 0 })
        }
        #[cfg(not(discord_social_sdk))]
        {
            false
        }
    }
    fn tick(&mut self) -> Result<(), String> {
        #[cfg(discord_social_sdk)]
        if let Some(handle) = self.handle {
            return ffi::result(unsafe { ffi::ca_discord_tick(handle.as_ptr()) });
        }
        Ok(())
    }
    #[cfg(debug_assertions)]
    fn diagnostics_json(&self) -> Option<String> {
        #[cfg(discord_social_sdk)]
        {
            let Some(handle) = self.handle else {
                return self.last_diagnostics.clone();
            };
            let size =
                unsafe { ffi::ca_discord_diagnostics(handle.as_ptr(), std::ptr::null_mut(), 0) };
            if size == 0 || size > 8192 {
                return None;
            }
            let mut output = vec![0u8; size];
            let written = unsafe {
                ffi::ca_discord_diagnostics(handle.as_ptr(), output.as_mut_ptr().cast(), size)
            };
            if written != size || output.last() != Some(&0) {
                return None;
            }
            output.pop();
            String::from_utf8(output).ok()
        }
        #[cfg(not(discord_social_sdk))]
        {
            self.last_diagnostics.clone()
        }
    }
}

impl Drop for SocialSdkProvider {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_client_id_fails_safely() {
        let mut provider = SocialSdkProvider::new(Some("invalid".into()));
        assert!(provider.connect().unwrap_err().contains("Application ID"));
        assert!(!provider.is_connected());
    }
    #[cfg(not(discord_social_sdk))]
    #[test]
    fn sdkless_build_is_explicit_and_never_claims_connection() {
        let mut provider = SocialSdkProvider::new(Some("123456789012345678".into()));
        assert!(provider.connect().unwrap_err().contains("does not include"));
        assert!(!provider.is_connected());
        assert!(provider.clear_activity().is_ok());
    }
}
