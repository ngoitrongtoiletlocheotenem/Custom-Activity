use super::*;
use std::{
    ffi::{c_void, OsStr},
    mem::{size_of, zeroed},
    os::windows::ffi::OsStrExt,
    ptr::{null, null_mut},
};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::SIZE,
        UI::{
            Controls::IImageList,
            Shell::{
                IShellItemImageFactory, SHCreateItemFromParsingName, SHGetImageList,
                SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
            },
        },
    },
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{Com::*, LibraryLoader::*},
    UI::{Shell::*, WindowsAndMessaging::*},
};

struct Icon(HICON);
impl Drop for Icon {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                DestroyIcon(self.0);
            }
        }
    }
}
struct Bitmap(HBITMAP);
impl Drop for Bitmap {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                DeleteObject(self.0);
            }
        }
    }
}
struct Dc(HDC);
impl Drop for Dc {
    fn drop(&mut self) {
        unsafe {
            DeleteDC(self.0);
        }
    }
}
struct Selection(HDC, HGDIOBJ);
impl Drop for Selection {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.0, self.1);
        }
    }
}
struct Module(HMODULE);
impl Drop for Module {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.0);
        }
    }
}
struct Com(bool);
impl Com {
    fn initialize() -> Self {
        // The service owns a worker thread. An existing apartment (RPC_E_CHANGED_MODE)
        // may still be used, but must never be uninitialized by us.
        Self(unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) } >= 0)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn paths(path: &Path) -> Option<(Vec<u16>, Vec<u16>)> {
    let absolute = std::path::absolute(path).ok()?;
    let normal = wide(absolute.as_os_str());
    if normal[..normal.len() - 1].contains(&0) {
        return None;
    }
    let units: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    let extended = if units.starts_with(&[92, 92, 63, 92]) {
        normal.clone()
    } else if units.starts_with(&[92, 92]) {
        [
            wide(OsStr::new(r"\\?\UNC\"))[..8].to_vec(),
            units[2..].to_vec(),
            vec![0],
        ]
        .concat()
    } else {
        [vec![92, 92, 63, 92], units, vec![0]].concat()
    };
    Some((normal, extended))
}

struct Candidate {
    image: RgbaImage,
    source: String,
    order: usize,
    depth: u16,
}
struct Collector {
    candidates: Vec<Candidate>,
    attempts: Vec<String>,
    generic: Vec<RgbaImage>,
}
impl Collector {
    fn add(&mut self, source: String, image: Option<RgbaImage>) {
        self.add_depth(source, image, 32);
    }
    fn add_depth(&mut self, source: String, image: Option<RgbaImage>, depth: u16) {
        let Some(image) = image.filter(meaningful) else {
            self.attempts.push(format!("{source}: failed/blank"));
            return;
        };
        if self
            .generic
            .iter()
            .any(|generic| same_icon(&image, generic))
        {
            self.attempts
                .push(format!("{source}: generic executable icon rejected"));
            return;
        }
        self.attempts.push(format!(
            "{source}: {}x{} RGBA",
            image.width(),
            image.height()
        ));
        // Retain only the best decoded image, rather than every resource from a
        // game with hundreds of icons. Exact embedded pixels beat Shell rendering
        // at equal content resolution/depth (Shell may flatten transparency).
        let exact = source.starts_with("PE/");
        if self.candidates.first().is_none_or(|best| {
            (quality(&image), depth, exact)
                > (
                    quality(&best.image),
                    best.depth,
                    best.source.starts_with("PE/"),
                )
        }) {
            self.candidates.clear();
            self.candidates.push(Candidate {
                image,
                source,
                order: self.attempts.len(),
                depth,
            });
        }
    }
}
fn same_icon(a: &RgbaImage, b: &RgbaImage) -> bool {
    let a = image::imageops::resize(a, 32, 32, image::imageops::FilterType::Triangle);
    let b = image::imageops::resize(b, 32, 32, image::imageops::FilterType::Triangle);
    let distance: u64 = a
        .pixels()
        .zip(b.pixels())
        .map(|(a, b)| {
            // Compare premultiplied colors, ignoring hidden RGB values under alpha zero.
            let color: u64 = (0..3)
                .map(|i| {
                    ((a[i] as i32 * a[3] as i32 / 255) - (b[i] as i32 * b[3] as i32 / 255))
                        .unsigned_abs() as u64
                })
                .sum();
            color + a[3].abs_diff(b[3]) as u64
        })
        .sum();
    distance < 32 * 32 * 4 * 3
}
fn quality(image: &RgbaImage) -> u64 {
    // Shell jumbo sometimes pads a 32px icon inside a 256px transparent canvas.
    // Prefer actual content resolution rather than the canvas in that case.
    let (mut left, mut top, mut right, mut bottom) = (image.width(), image.height(), 0, 0);
    for (x, y, p) in image.enumerate_pixels() {
        if p[3] > 8 {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }
    u64::from(right.saturating_sub(left).min(bottom.saturating_sub(top)))
}

pub(super) fn extract(path: &Path, diagnostic: bool) -> Result<ExtractedIcon, IconExtractionError> {
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_millis(if diagnostic { 5000 } else { 750 });
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return Err(IconExtractionError(
            "Executable does not exist or is inaccessible".into(),
        ));
    }
    let (normal, extended) =
        paths(path).ok_or_else(|| IconExtractionError("Invalid executable path".into()))?;
    let _com = Com::initialize();
    let mut collector = Collector {
        candidates: Vec::new(),
        attempts: Vec::new(),
        generic: Vec::new(),
    };
    unsafe {
        // Compare against the current Windows theme's generic .exe icon, not a
        // hard-coded screenshot. USEFILEATTRIBUTES never opens this synthetic name.
        let mut generic: SHFILEINFOW = zeroed();
        let generic_path = wide(OsStr::new("CurrentActivity-missing-generic-probe.exe"));
        let generic_ok = SHGetFileInfoW(
            generic_path.as_ptr(),
            0x80,
            &mut generic,
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON | SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES,
        ) != 0;
        if generic_ok {
            let icon = Icon(generic.hIcon);
            if let Some(image) = icon_image(icon.0) {
                collector.generic.push(image);
            }
        }
        let shared = LoadIconW(null_mut(), IDI_APPLICATION);
        if let Some(image) = icon_image(shared) {
            collector.generic.push(image);
        }
        let mut info: SHFILEINFOW = zeroed();
        let mut shell_ok = false;
        for pathname in [&normal, &extended] {
            if SHGetFileInfoW(
                pathname.as_ptr(),
                0,
                &mut info,
                size_of::<SHFILEINFOW>() as u32,
                SHGFI_SYSICONINDEX,
            ) != 0
            {
                shell_ok = true;
                break;
            }
        }
        for (kind, label) in [
            (4, "Shell/Jumbo"),
            (2, "Shell/ExtraLarge"),
            (0, "Shell/Large"),
        ] {
            if generic_ok {
                if let Some(image) = SHGetImageList::<IImageList>(kind)
                    .ok()
                    .and_then(|list| list.GetIcon(generic.iIcon, 1).ok())
                    .and_then(|icon| {
                        let owned = Icon(icon.0);
                        icon_image(owned.0)
                    })
                {
                    collector.generic.push(image);
                }
            }
            if shell_ok && generic_ok && info.iIcon == generic.iIcon {
                collector.attempts.push(format!(
                    "{label}: generic executable image-list index rejected"
                ));
                continue;
            }
            let result = if shell_ok {
                SHGetImageList::<IImageList>(kind)
                    .ok()
                    .and_then(|list| list.GetIcon(info.iIcon, 1).ok())
                    .and_then(|icon| {
                        let owned = Icon(icon.0);
                        icon_image(owned.0)
                    })
            } else {
                None
            };
            collector.add(label.into(), result);
        }
        let mut factory_image = None;
        for pathname in [&normal, &extended] {
            if let Ok(factory) = SHCreateItemFromParsingName::<_, _, IShellItemImageFactory>(
                PCWSTR(pathname.as_ptr()),
                None,
            ) {
                if let Ok(bitmap) = factory.GetImage(
                    SIZE { cx: 256, cy: 256 },
                    SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
                ) {
                    let owned = Bitmap(bitmap.0);
                    factory_image = bitmap_image(owned.0);
                    if factory_image.is_some() {
                        break;
                    }
                }
            }
        }
        collector.add("IShellItemImageFactory".into(), factory_image);
        let count = ExtractIconExW(extended.as_ptr(), -1, null_mut(), null_mut(), 0)
            .min(if diagnostic { 16 } else { 4 });
        // PrivateExtractIcons can succeed even when ExtractIconEx's count query fails.
        for size in [256, 128, 64, 48, 32] {
            for index in 0..count.max(1) {
                // Every requested size still gets its primary group attempt.
                // Extra groups are best-effort within the overall effort budget.
                if index > 0 && std::time::Instant::now() >= deadline {
                    break;
                }
                let mut result = None;
                for pathname in [&extended, &normal] {
                    let mut handle = null_mut();
                    let mut id = 0;
                    let extracted = PrivateExtractIconsW(
                        pathname.as_ptr(),
                        index as i32,
                        size,
                        size,
                        &mut handle,
                        &mut id,
                        1,
                        0,
                    );
                    let owned = Icon(handle);
                    if extracted != 0 && extracted != u32::MAX {
                        result = icon_image(owned.0);
                    }
                    if result.is_some() {
                        break;
                    }
                }
                collector.add(format!("PrivateExtractIconsW/{size}/group{index}"), result);
            }
        }
        for index in 0..count.max(1) {
            if index > 0 && std::time::Instant::now() >= deadline {
                break;
            }
            let mut large = null_mut();
            let mut small = null_mut();
            ExtractIconExW(extended.as_ptr(), index as i32, &mut large, &mut small, 1);
            let (large, small) = (Icon(large), Icon(small));
            collector.add(
                format!("ExtractIconExW/group{index}/large"),
                icon_image(large.0),
            );
            collector.add(
                format!("ExtractIconExW/group{index}/small"),
                icon_image(small.0),
            );
        }
        resources(&extended, &mut collector, diagnostic, deadline);
    }
    let selected = collector
        .candidates
        .into_iter()
        .max_by_key(|candidate| {
            (
                quality(&candidate.image),
                candidate.depth,
                std::cmp::Reverse(candidate.order),
            )
        })
        .ok_or_else(|| {
            IconExtractionError(format!(
                "No app-specific icon extracted: {}",
                collector.attempts.join("; ")
            ))
        })?;
    Ok(ExtractedIcon {
        original_png: png(&selected.image)?,
        png: bounded_png(&selected.image)?,
        width: selected.image.width(),
        height: selected.image.height(),
        source: selected.source,
        attempts: collector.attempts,
    })
}

unsafe fn bitmap_dimensions(bitmap: HBITMAP) -> Option<(u32, u32)> {
    let mut info: BITMAP = zeroed();
    if GetObjectW(
        bitmap,
        size_of::<BITMAP>() as i32,
        &mut info as *mut _ as *mut c_void,
    ) == 0
    {
        return None;
    }
    let (width, height) = (
        u32::try_from(info.bmWidth).ok()?,
        info.bmHeight.unsigned_abs(),
    );
    (width > 0 && height > 0 && width <= 1024 && height <= 2048).then_some((width, height))
}
unsafe fn read_bitmap(bitmap: HBITMAP, width: u32, height: u32) -> Option<Vec<u8>> {
    let dc = Dc(CreateCompatibleDC(null_mut()));
    if dc.0.is_null() {
        return None;
    }
    let mut header: BITMAPINFO = zeroed();
    header.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    header.bmiHeader.biWidth = width as i32;
    header.bmiHeader.biHeight = -(height as i32);
    header.bmiHeader.biPlanes = 1;
    header.bmiHeader.biBitCount = 32;
    header.bmiHeader.biCompression = BI_RGB;
    let mut bytes = vec![0u8; width as usize * height as usize * 4];
    (GetDIBits(
        dc.0,
        bitmap,
        0,
        height,
        bytes.as_mut_ptr() as *mut c_void,
        &mut header,
        DIB_RGB_COLORS,
    ) == height as i32)
        .then_some(bytes)
}
unsafe fn bitmap_image(bitmap: HBITMAP) -> Option<RgbaImage> {
    let (width, height) = bitmap_dimensions(bitmap)?;
    if height > 1024 {
        return None;
    }
    let pixels = read_bitmap(bitmap, width, height)?;
    // Factory HBITMAP has no AND mask to recover old-style transparency. Never
    // invent an opaque rectangle when its alpha channel is entirely absent.
    if pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
        return None;
    }
    RgbaImage::from_raw(width, height, rgba_from_bgra(pixels, None))
}
unsafe fn icon_image(icon: HICON) -> Option<RgbaImage> {
    if icon.is_null() {
        return None;
    }
    let mut info: ICONINFO = zeroed();
    if GetIconInfo(icon, &mut info) == 0 {
        return None;
    }
    let (color, mask) = (Bitmap(info.hbmColor), Bitmap(info.hbmMask));
    let (width, height) = if !color.0.is_null() {
        bitmap_dimensions(color.0)?
    } else {
        let (w, h) = bitmap_dimensions(mask.0)?;
        (w, h / 2)
    };
    if width == 0 || height == 0 || width > 1024 || height > 1024 {
        return None;
    }
    let dc = Dc(CreateCompatibleDC(null_mut()));
    if dc.0.is_null() {
        return None;
    }
    let mut header: BITMAPINFO = zeroed();
    header.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    header.bmiHeader.biWidth = width as i32;
    header.bmiHeader.biHeight = -(height as i32);
    header.bmiHeader.biPlanes = 1;
    header.bmiHeader.biBitCount = 32;
    header.bmiHeader.biCompression = BI_RGB;
    let mut bits = null_mut();
    let bitmap = Bitmap(CreateDIBSection(
        dc.0,
        &header,
        DIB_RGB_COLORS,
        &mut bits,
        null_mut(),
        0,
    ));
    if bitmap.0.is_null() || bits.is_null() {
        return None;
    }
    let old = SelectObject(dc.0, bitmap.0);
    if old.is_null() || old as isize == -1 {
        return None;
    }
    let selection = Selection(dc.0, old);
    std::ptr::write_bytes(bits, 0, width as usize * height as usize * 4);
    if DrawIconEx(
        dc.0,
        0,
        0,
        icon,
        width as i32,
        height as i32,
        0,
        null_mut(),
        DI_NORMAL,
    ) == 0
    {
        return None;
    }
    GdiFlush();
    let pixels =
        std::slice::from_raw_parts(bits as *const u8, width as usize * height as usize * 4)
            .to_vec();
    drop(selection);
    let transparent = if !mask.0.is_null() {
        let (_, mask_height) = bitmap_dimensions(mask.0)?;
        read_bitmap(mask.0, width, mask_height).map(|bytes| {
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .take((width * height) as usize)
                .map(|p| p[0] > 127)
                .collect::<Vec<_>>()
        })
    } else {
        None
    };
    RgbaImage::from_raw(
        width,
        height,
        rgba_from_bgra(pixels, transparent.as_deref()),
    )
}

enum ResourceName {
    Id(usize),
    Text(Vec<u16>),
}
impl ResourceName {
    fn pointer(&self) -> *const u16 {
        match self {
            Self::Id(id) => *id as *const u16,
            Self::Text(text) => text.as_ptr(),
        }
    }
}
unsafe extern "system" fn collect_resource(
    _module: HMODULE,
    _kind: *const u16,
    name: *const u16,
    data: isize,
) -> i32 {
    let names = &mut *(data as *mut Vec<ResourceName>);
    if names.len() >= 256 {
        return 0;
    }
    if (name as usize) >> 16 == 0 {
        names.push(ResourceName::Id(name as usize));
    } else {
        let mut text = Vec::new();
        for index in 0..1024 {
            let unit = *name.add(index);
            text.push(unit);
            if unit == 0 {
                names.push(ResourceName::Text(text));
                return 1;
            }
        }
    }
    1
}
unsafe fn resource_bytes(module: &Module, name: *const u16, kind: usize) -> Option<&[u8]> {
    let resource = FindResourceW(module.0, name, kind as *const u16);
    if resource.is_null() {
        return None;
    }
    let size = SizeofResource(module.0, resource) as usize;
    if size == 0 || size > MAX_RESOURCE_BYTES {
        return None;
    }
    let loaded = LoadResource(module.0, resource);
    if loaded.is_null() {
        return None;
    }
    let data = LockResource(loaded);
    if data.is_null() {
        return None;
    }
    Some(std::slice::from_raw_parts(data as *const u8, size))
}
unsafe fn resources(
    path: &[u16],
    collector: &mut Collector,
    diagnostic: bool,
    deadline: std::time::Instant,
) {
    // These flags map resources only: DllMain/import initialization never runs.
    let module = Module(LoadLibraryExW(
        path.as_ptr(),
        null_mut(),
        LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
    ));
    if module.0.is_null() {
        collector
            .attempts
            .push("PE/resources: resource-only load failed".into());
        return;
    }
    let mut groups = Vec::<ResourceName>::new();
    EnumResourceNamesW(
        module.0,
        14usize as *const u16,
        Some(collect_resource),
        &mut groups as *mut _ as isize,
    );
    if groups.is_empty() {
        collector
            .attempts
            .push("PE/resources: no icon groups".into());
    }
    let mut budget = if diagnostic { 32 } else { 8 } * 1024 * 1024usize;
    let mut frames = Vec::new();
    for (group_index, name) in groups.iter().enumerate() {
        let Some(group) = resource_bytes(&module, name.pointer(), 14) else {
            continue;
        };
        if group.len() < 6 || group[0..4] != [0, 0, 1, 0] {
            continue;
        }
        let count = u16::from_le_bytes([group[4], group[5]]) as usize;
        if count > 256 || group.len() < 6 + count * 14 {
            continue;
        }
        for frame in 0..count {
            let entry = &group[6 + frame * 14..6 + (frame + 1) * 14];
            let width = if entry[0] == 0 { 256 } else { entry[0] as u16 };
            let height = if entry[1] == 0 { 256 } else { entry[1] as u16 };
            let depth = u16::from_le_bytes([entry[6], entry[7]]).min(32);
            frames.push((width.min(height), depth, group_index, frame));
        }
    }
    // Metadata inspection is cheap; only decode the most promising resource
    // frames, rather than thousands of low-resolution/color variants.
    frames.sort_by_key(|&(size, depth, group, frame)| {
        (
            std::cmp::Reverse(size),
            std::cmp::Reverse(depth),
            group,
            frame,
        )
    });
    let limit = if diagnostic { 64 } else { 12 };
    for (attempt, (_, depth, group_index, frame)) in frames.into_iter().take(limit).enumerate() {
        if attempt > 0 && std::time::Instant::now() >= deadline {
            collector
                .attempts
                .push("PE/resources: extraction effort deadline reached".into());
            break;
        }
        let Some(group) = resource_bytes(&module, groups[group_index].pointer(), 14) else {
            continue;
        };
        let entry = &group[6 + frame * 14..6 + (frame + 1) * 14];
        let id = u16::from_le_bytes([entry[12], entry[13]]) as usize;
        let source = format!("PE/RT_GROUP_ICON#{group_index}/frame{frame}/RT_ICON#{id}");
        let image = resource_bytes(&module, id as *const u16, 3).and_then(|bytes| {
            if bytes.len() > budget {
                return None;
            }
            budget -= bytes.len();
            decode_resource(group, frame, bytes)
        });
        collector.add_depth(source, image, depth);
        if budget == 0 {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_unicode_paths_remain_utf16_and_unc_prefix_is_valid() {
        let path =
            std::path::PathBuf::from(format!("C:\\{}\\界 ゲーム.exe", "directory\\".repeat(40)));
        let (_, extended) = paths(&path).unwrap();
        assert!(extended.starts_with(&[92, 92, 63, 92]));
        assert_eq!(extended.last(), Some(&0));
        assert!(extended.windows(1).any(|units| units[0] == '界' as u16));
        let (_, unc) = paths(Path::new(r"\\server\share\界.exe")).unwrap();
        assert!(unc.starts_with(&[92, 92, 63, 92, 85, 78, 67, 92]));
    }
    #[test]
    fn rejects_generic_and_prefers_actual_content_over_padded_jumbo() {
        let generic = RgbaImage::from_fn(32, 32, |x, y| {
            image::Rgba([x as u8 * 7, y as u8 * 7, 0, 255])
        });
        let mut collector = Collector {
            candidates: Vec::new(),
            attempts: Vec::new(),
            generic: vec![generic.clone()],
        };
        collector.add("generic".into(), Some(generic));
        assert!(collector.candidates.is_empty());
        let mut padded = RgbaImage::new(256, 256);
        for y in 100..132 {
            for x in 100..132 {
                padded.put_pixel(x, y, image::Rgba([255, 0, 255, 255]));
            }
        }
        let actual = RgbaImage::from_fn(64, 64, |x, y| {
            image::Rgba([x as u8 * 3, 0, y as u8 * 3, 255])
        });
        collector.add("padded shell jumbo".into(), Some(padded));
        collector.add("embedded actual64".into(), Some(actual));
        assert_eq!(collector.candidates[0].source, "embedded actual64");
    }
    #[test]
    fn exact_resource_beats_shell_at_equal_quality() {
        let image = RgbaImage::from_fn(32, 32, |x, y| {
            image::Rgba([x as u8 * 7, 0, y as u8 * 7, if x == 0 { 0 } else { 255 }])
        });
        let mut collector = Collector {
            candidates: Vec::new(),
            attempts: Vec::new(),
            generic: Vec::new(),
        };
        collector.add("Shell/Jumbo".into(), Some(image.clone()));
        collector.add("PE/RT_GROUP_ICON#0/frame0".into(), Some(image));
        assert!(collector.candidates[0].source.starts_with("PE/"));
    }
}
