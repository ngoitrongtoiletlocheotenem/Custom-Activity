//! Local executable images only. No process execution, content inspection or network I/O.
#[cfg(windows)]
mod native;

use image::{
    codecs::png::{CompressionType, FilterType, PngEncoder},
    ImageEncoder, RgbaImage,
};
use std::{fmt, path::Path};

pub const CACHE_VERSION: &[u8] = b"currentactivity-icon-v3\0";
const MAX_RESOURCE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct ExtractedIcon {
    /// Highest-quality original, retained locally; never sent directly to hosting.
    pub original_png: Vec<u8>,
    /// Validated PNG fitting the existing hosting contract; no forced upscaling.
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub source: String,
    pub attempts: Vec<String>,
}

#[derive(Debug)]
pub struct IconExtractionError(pub String);
impl fmt::Display for IconExtractionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for IconExtractionError {}

pub fn extract_application_icon(path: &Path) -> Result<ExtractedIcon, IconExtractionError> {
    #[cfg(windows)]
    {
        native::extract(path, false)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(IconExtractionError(
            "Icon extraction requires Windows".into(),
        ))
    }
}

/// Run broader, explicitly requested diagnostics without affecting normal scans.
#[cfg(debug_assertions)]
pub fn diagnose_application_icon(path: &Path) -> Result<ExtractedIcon, IconExtractionError> {
    #[cfg(windows)]
    {
        native::extract(path, true)
    }
    #[cfg(not(windows))]
    {
        extract_application_icon(path)
    }
}

fn png(image: &RgbaImage) -> Result<Vec<u8>, IconExtractionError> {
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Best, FilterType::Adaptive)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|_| IconExtractionError("PNG encoding failed".into()))?;
    Ok(bytes)
}

pub(crate) fn validated_png(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > 128 * 1024 {
        return false;
    }
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(256);
    limits.max_image_height = Some(256);
    limits.max_alloc = Some(1024 * 1024);
    reader.limits(limits);
    let Ok(image) = reader.decode() else {
        return false;
    };
    image.width() <= 256 && image.height() <= 256 && meaningful(&image.to_rgba8())
}

fn meaningful(image: &RgbaImage) -> bool {
    if image.width() == 0 || image.height() == 0 || image.width() > 1024 || image.height() > 1024 {
        return false;
    }
    let mut visible = 0;
    let first = image.pixels().next().map(|p| p.0);
    let mut varied = false;
    for pixel in image.pixels() {
        if pixel[3] > 8 {
            visible += 1;
        }
        varied |= Some(pixel.0) != first;
    }
    visible >= 4 && varied
}

fn bounded_png(original: &RgbaImage) -> Result<Vec<u8>, IconExtractionError> {
    let mut current = if original.width().max(original.height()) > 256 {
        image::imageops::resize(
            original,
            (original.width() * 256 / original.width().max(original.height())).max(1),
            (original.height() * 256 / original.width().max(original.height())).max(1),
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        original.clone()
    };
    loop {
        let encoded = png(&current)?;
        if encoded.len() <= 128 * 1024 {
            return Ok(encoded);
        }
        // Preserve RGBA and use lossless PNG; reduce dimensions only if compression alone cannot fit.
        if current.width().min(current.height()) <= 16 {
            return Err(IconExtractionError("Icon exceeds hosting limits".into()));
        }
        current = image::imageops::resize(
            &current,
            (current.width() * 7 / 8).max(1),
            (current.height() * 7 / 8).max(1),
            image::imageops::FilterType::Lanczos3,
        );
    }
}

/// Reconstruct one ICO frame from a bounded RT_GROUP_ICON entry + exact RT_ICON bytes.
fn resource_ico(group: &[u8], frame: usize, resource: &[u8]) -> Option<Vec<u8>> {
    if group.len() < 6 || group[0..4] != [0, 0, 1, 0] {
        return None;
    }
    let count = u16::from_le_bytes(group[4..6].try_into().ok()?) as usize;
    if count == 0 || count > 256 || frame >= count || group.len() < 6 + count * 14 {
        return None;
    }
    let entry = &group[6 + frame * 14..6 + (frame + 1) * 14];
    let declared = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
    if declared == 0 || declared > MAX_RESOURCE_BYTES || declared != resource.len() {
        return None;
    }
    let mut ico = Vec::with_capacity(22 + declared);
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    ico.extend_from_slice(&entry[..12]);
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(resource);
    Some(ico)
}

fn decode_resource(group: &[u8], frame: usize, resource: &[u8]) -> Option<RgbaImage> {
    let ico = resource_ico(group, frame, resource)?;
    let mut decoder =
        image::ImageReader::with_format(std::io::Cursor::new(ico), image::ImageFormat::Ico);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1024);
    limits.max_image_height = Some(1024);
    limits.max_alloc = Some(16 * 1024 * 1024);
    decoder.limits(limits);
    let decoded = decoder.decode().ok()?.to_rgba8();
    meaningful(&decoded).then_some(decoded)
}

/// GDI returns premultiplied BGRA. Older HICONs instead use their AND mask.
fn rgba_from_bgra(mut pixels: Vec<u8>, transparent: Option<&[bool]>) -> Vec<u8> {
    let alpha_present = pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
    for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        pixel.swap(0, 2);
        if alpha_present {
            let alpha = pixel[3] as u32;
            for channel in &mut pixel[..3] {
                *channel = (*channel as u32 * 255 + alpha / 2)
                    .checked_div(alpha)
                    .unwrap_or(0)
                    .min(255) as u8;
            }
        } else {
            pixel[3] = if transparent.is_some_and(|mask| mask.get(index).copied().unwrap_or(false))
            {
                0
            } else {
                255
            };
            if pixel[3] == 0 {
                pixel[..3].fill(0);
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    fn patterned() -> RgbaImage {
        RgbaImage::from_fn(32, 32, |x, y| {
            image::Rgba([x as u8 * 7, y as u8 * 7, 50, if x < 2 { 0 } else { 255 }])
        })
    }
    fn group(resource: &[u8]) -> Vec<u8> {
        let mut group = vec![0, 0, 1, 0, 1, 0, 32, 32, 0, 0, 1, 0, 32, 0];
        group.extend_from_slice(&(resource.len() as u32).to_le_bytes());
        group.extend_from_slice(&1u16.to_le_bytes());
        group
    }
    #[test]
    fn reconstructs_png_and_rejects_truncation_and_size_mismatch() {
        let source = patterned();
        let bytes = png(&source).unwrap();
        let group = group(&bytes);
        assert_eq!(decode_resource(&group, 0, &bytes).unwrap(), source);
        assert!(resource_ico(&group[..12], 0, &bytes).is_none());
        assert!(resource_ico(&group, 1, &bytes).is_none());
        assert!(resource_ico(&group, 0, &bytes[..bytes.len() - 1]).is_none());
    }
    #[test]
    fn decodes_legacy_dib_with_black_opaque_and_transparent_mask() {
        // 2x2 24-bit bottom-up XOR bitmap followed by aligned 1-bit AND mask.
        let mut dib = vec![0u8; 40];
        dib[0..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&2i32.to_le_bytes());
        dib[8..12].copy_from_slice(&4i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24u16.to_le_bytes());
        dib.extend_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0]);
        dib.extend_from_slice(&[0, 0, 0, 0, 0x40, 0, 0, 0]);
        let mut header = group(&dib);
        header[6] = 2;
        header[7] = 2;
        header[12..14].copy_from_slice(&24u16.to_le_bytes());
        let ico = resource_ico(&header, 0, &dib).unwrap();
        let decoded = image::load_from_memory_with_format(&ico, image::ImageFormat::Ico)
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_eq!(decoded.get_pixel(1, 0)[3], 0);
    }
    #[test]
    fn mask_keeps_black_opaque_and_alpha_unpremultiplies() {
        assert_eq!(
            rgba_from_bgra(vec![0, 0, 0, 0, 255, 255, 255, 0], Some(&[false, true])),
            [0, 0, 0, 255, 0, 0, 0, 0]
        );
        assert_eq!(
            rgba_from_bgra(vec![0, 32, 64, 128], None),
            [128, 64, 0, 128]
        );
    }
    #[test]
    fn rejects_blank_and_corrupt_cache_images() {
        assert!(!meaningful(&RgbaImage::new(32, 32)));
        assert!(!meaningful(&RgbaImage::from_pixel(
            32,
            32,
            image::Rgba([255, 255, 255, 255])
        )));
        assert!(!validated_png(b"not a PNG"));
        assert!(validated_png(&bounded_png(&patterned()).unwrap()));
    }
    #[test]
    fn noisy_rgba_fits_upload_contract_without_forced_upscale() {
        let noisy = RgbaImage::from_fn(256, 256, |x, y| {
            let n = x
                .wrapping_mul(1664525)
                .wrapping_add(y.wrapping_mul(1013904223));
            image::Rgba([(n >> 8) as u8, (n >> 16) as u8, n as u8, 255])
        });
        assert!(validated_png(&bounded_png(&noisy).unwrap()));
        let small = image::load_from_memory(&bounded_png(&patterned()).unwrap()).unwrap();
        assert_eq!((small.width(), small.height()), (32, 32));
    }
}
