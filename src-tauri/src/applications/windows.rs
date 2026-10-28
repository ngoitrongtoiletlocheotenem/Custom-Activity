//! Process metadata only. This module deliberately never reads window titles.
use super::{application_id, normalize_name, normalized_identity, DetectedApplication};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    ffi::c_void,
    io::Cursor,
    mem::{size_of, zeroed},
    path::Path,
    ptr::null_mut,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    Storage::FileSystem::*,
    System::{Diagnostics::ToolHelp::*, Threading::*},
    UI::{Shell::*, WindowsAndMessaging::*},
};

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let _guard = OwnedHandle(handle);
        let mut buffer = vec![0u16; 32768];
        let mut len = buffer.len() as u32;
        if QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut len) == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buffer[..len as usize]))
    }
}

fn processes() -> Vec<(u32, String)> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let _guard = OwnedHandle(snapshot);
        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut result = Vec::new();
        let mut ok = Process32FirstW(snapshot, &mut entry);
        while ok != 0 {
            let end = entry
                .szExeFile
                .iter()
                .position(|v| *v == 0)
                .unwrap_or(entry.szExeFile.len());
            result.push((
                entry.th32ProcessID,
                String::from_utf16_lossy(&entry.szExeFile[..end]),
            ));
            ok = Process32NextW(snapshot, &mut entry);
        }
        result
    }
}
pub fn running(path: &str) -> bool {
    let wanted = normalized_identity(path);
    let filename = wanted.rsplit('\\').next().unwrap_or("");
    processes()
        .into_iter()
        .filter(|(_, name)| name.eq_ignore_ascii_case(filename))
        .any(|(pid, _)| process_path(pid).is_some_and(|p| normalized_identity(&p) == wanted))
}
pub fn discord_running() -> bool {
    processes().iter().any(|(_, name)| {
        ["Discord.exe", "DiscordCanary.exe", "DiscordPTB.exe"]
            .iter()
            .any(|expected| name.eq_ignore_ascii_case(expected))
    })
}

unsafe extern "system" fn collect_window(hwnd: HWND, data: LPARAM) -> BOOL {
    if eligible(hwnd) {
        (*(data as *mut Vec<u32>)).push(window_pid(hwnd));
    }
    1
}
unsafe fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    pid
}
unsafe fn eligible(hwnd: HWND) -> bool {
    if hwnd == GetShellWindow()
        || IsWindowVisible(hwnd) == 0
        || !GetWindow(hwnd, GW_OWNER).is_null()
        || GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW as isize != 0
    {
        return false;
    }
    // Window class identifies shell infrastructure, never user content.
    let mut class_buffer = [0u16; 256];
    let class_length = GetClassNameW(hwnd, class_buffer.as_mut_ptr(), class_buffer.len() as i32);
    if class_length > 0 {
        let class_name = String::from_utf16_lossy(&class_buffer[..class_length as usize]);
        if [
            "Shell_TrayWnd",
            "Shell_SecondaryTrayWnd",
            "Progman",
            "WorkerW",
        ]
        .contains(&class_name.as_str())
        {
            return false;
        }
    }
    let mut cloaked: u32 = 0;
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED as u32,
        &mut cloaked as *mut _ as *mut c_void,
        size_of::<u32>() as u32,
    ) >= 0
        && cloaked != 0
    {
        return false;
    }
    let pid = window_pid(hwnd);
    pid != 0 && pid != GetCurrentProcessId()
}
pub fn list(cache: &Path) -> Result<Vec<DetectedApplication>, String> {
    let mut pids: Vec<u32> = Vec::new();
    unsafe {
        if EnumWindows(Some(collect_window), &mut pids as *mut _ as LPARAM) == 0 {
            return Err("Unable to enumerate application windows".into());
        }
    }
    pids.sort_unstable();
    pids.dedup();
    Ok(pids
        .into_iter()
        .filter_map(|pid| application(pid, cache))
        .collect())
}
pub fn foreground(cache: &Path) -> Result<Option<DetectedApplication>, String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() || !eligible(hwnd) {
            return Ok(None);
        }
        Ok(application(window_pid(hwnd), cache))
    }
}
fn application(pid: u32, cache: &Path) -> Option<DetectedApplication> {
    let path = process_path(pid)?;
    let executable = Path::new(&path).file_name()?.to_string_lossy().into_owned();
    if [
        "svchost.exe",
        "runtimebroker.exe",
        "conhost.exe",
        "dllhost.exe",
        "dwm.exe",
        "sihost.exe",
        "shellexperiencehost.exe",
        "searchhost.exe",
        "searchapp.exe",
        "startmenuexperiencehost.exe",
        "textinputhost.exe",
        "applicationframehost.exe",
    ]
    .contains(&executable.to_lowercase().as_str())
    {
        return None;
    }
    let (product, description) = version_names(&path);
    Some(DetectedApplication {
        id: application_id(&path),
        name: normalize_name(product.as_deref(), description.as_deref(), &executable),
        executable_name: executable,
        icon_data_url: cached_icon(&path, cache),
        executable_path: path,
        process_ids: vec![pid],
        running: true,
    })
}

fn version_names(path: &str) -> (Option<String>, Option<String>) {
    unsafe {
        let path = wide(path);
        let mut ignored = 0;
        let len = GetFileVersionInfoSizeW(path.as_ptr(), &mut ignored);
        if len == 0 || len > 4 * 1024 * 1024 {
            return (None, None);
        }
        let mut data = vec![0u8; len as usize];
        if GetFileVersionInfoW(path.as_ptr(), 0, len, data.as_mut_ptr() as *mut c_void) == 0 {
            return (None, None);
        }
        let mut value: *mut c_void = null_mut();
        let mut size = 0;
        let translation = wide("\\VarFileInfo\\Translation");
        let pair = if VerQueryValueW(
            data.as_ptr() as *const c_void,
            translation.as_ptr(),
            &mut value,
            &mut size,
        ) != 0
            && size >= 4
            && !value.is_null()
        {
            let pair = std::slice::from_raw_parts(value as *const u16, 2);
            (pair[0], pair[1])
        } else {
            (0x0409, 0x04b0)
        };
        let query = |field: &str| -> Option<String> {
            let key = wide(&format!(
                "\\StringFileInfo\\{:04x}{:04x}\\{}",
                pair.0, pair.1, field
            ));
            let mut value: *mut c_void = null_mut();
            let mut size = 0;
            if VerQueryValueW(
                data.as_ptr() as *const c_void,
                key.as_ptr(),
                &mut value,
                &mut size,
            ) == 0
                || value.is_null()
                || size == 0
            {
                return None;
            }
            let text = std::slice::from_raw_parts(value as *const u16, (size as usize).min(4096));
            Some(String::from_utf16_lossy(
                &text[..text.iter().position(|c| *c == 0).unwrap_or(text.len())],
            ))
        };
        (query("ProductName"), query("FileDescription"))
    }
}

fn cached_icon(path: &str, cache: &Path) -> Option<String> {
    // Timestamp/size make replacement executables invalidate disposable cached icons.
    let file = super::icon_cache_file(path, cache)?;
    if let Ok(bytes) = std::fs::read(&file) {
        if bytes.len() <= 128 * 1024 {
            return Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes)));
        }
    }
    let bytes = extract_icon(path)?;
    if std::fs::create_dir_all(cache).is_ok() {
        // At most 256 files; remove oldest entries before adding another.
        if let Ok(entries) = std::fs::read_dir(cache) {
            let mut entries: Vec<_> = entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
                .collect();
            entries.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
            let remove = entries.len().saturating_sub(255);
            for entry in entries.into_iter().take(remove) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        let _ = std::fs::write(file, &bytes);
    }
    Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

fn extract_icon(path: &str) -> Option<Vec<u8>> {
    unsafe {
        let mut info: SHFILEINFOW = zeroed();
        if SHGetFileInfoW(
            wide(path).as_ptr(),
            0,
            &mut info,
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        ) == 0
            || info.hIcon.is_null()
        {
            return None;
        }
        let icon = info.hIcon;
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            DestroyIcon(icon);
            return None;
        }
        let mut bitmap_info: BITMAPINFO = zeroed();
        bitmap_info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bitmap_info.bmiHeader.biWidth = 64;
        bitmap_info.bmiHeader.biHeight = -64;
        bitmap_info.bmiHeader.biPlanes = 1;
        bitmap_info.bmiHeader.biBitCount = 32;
        bitmap_info.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(dc, &bitmap_info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(dc);
            DestroyIcon(icon);
            return None;
        }
        let old = SelectObject(dc, bitmap);
        std::ptr::write_bytes(bits, 0, 64 * 64 * 4);
        let drawn = DrawIconEx(dc, 0, 0, icon, 64, 64, 0, null_mut(), DI_NORMAL);
        let mut pixels = std::slice::from_raw_parts(bits as *const u8, 64 * 64 * 4).to_vec();
        SelectObject(dc, old);
        DeleteObject(bitmap);
        DeleteDC(dc);
        DestroyIcon(icon);
        if drawn == 0 {
            return None;
        }
        let (rgba_pixels, _) = pixels.as_chunks_mut::<4>();
        for pixel in rgba_pixels.iter_mut() {
            pixel.swap(0, 2);
        }
        // Older icons use a mask rather than alpha; keep their opaque RGB visible.
        if rgba_pixels.iter().all(|p| p[3] == 0) {
            for pixel in rgba_pixels.iter_mut() {
                if pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0 {
                    pixel[3] = 255;
                }
            }
        }
        let image = image::RgbaImage::from_raw(64, 64, pixels)?;
        let mut output = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut output, image::ImageFormat::Png)
            .ok()?;
        Some(output.into_inner())
    }
}
