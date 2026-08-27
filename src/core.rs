//! Decode, scale, encode. No binding layer, no platform assumptions.
//!
//! Both the native addon and the WebAssembly build call straight into this, so
//! whatever difference a benchmark shows between them is the runtime, not two
//! different implementations being compared by accident.

use std::io::Cursor;

use fast_image_resize::images::Image as FirImage;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType as PngCompression, FilterType as PngFilter, PngEncoder};
use image::{DynamicImage, ExtendedColorType, ImageEncoder, ImageReader};

/// Ceiling on decoded pixels, enforced from the header before any decoder
/// allocates. 100 MP is ~400 MB of RGBA, well above any real scan.
pub const MAX_PIXELS: u64 = 100_000_000;

#[derive(Clone, Copy)]
pub struct ResizeSpec {
    pub width: u32,
    pub height: u32,
    pub filter: FilterType,
}

#[derive(Clone, Copy)]
pub enum Output {
    /// quality 1..=100
    Jpeg(u8),
    Png(PngCompression),
    /// quality 1..=100. Native only; the wasm build routes WebP here as JPEG
    /// because libwebp is C and does not cross to wasm32-unknown-unknown.
    #[cfg(not(target_arch = "wasm32"))]
    Webp(f32),
}

pub fn filter_from_u8(v: u8) -> FilterType {
    match v {
        0 => FilterType::Box,
        1 => FilterType::Bilinear,
        2 => FilterType::Hamming,
        3 => FilterType::CatmullRom,
        4 => FilterType::Mitchell,
        _ => FilterType::Lanczos3,
    }
}

/// Header-only read: dimensions and container, no decoder constructed.
pub fn dimensions(input: &[u8]) -> Result<(u32, u32, String), String> {
    let reader = ImageReader::new(Cursor::new(input))
        .with_guessed_format()
        .map_err(|e| format!("unreadable image: {e}"))?;
    let format = reader
        .format()
        .map(|f| format!("{f:?}").to_lowercase())
        .unwrap_or_else(|| "unknown".to_string());
    let (w, h) = reader
        .into_dimensions()
        .map_err(|e| format!("no dimensions in header: {e}"))?;
    Ok((w, h, format))
}

fn guard_pixels(w: u32, h: u32) -> Result<(), String> {
    let pixels = u64::from(w) * u64::from(h);
    if pixels > MAX_PIXELS {
        return Err(format!(
            "image is {w}x{h} ({pixels} pixels), over the {MAX_PIXELS} pixel limit"
        ));
    }
    Ok(())
}

/// Decode a JPEG straight to a reduced size using a smaller IDCT.
///
/// The decoder rounds the request up to 1/1, 1/2, 1/4 or 1/8 of the source, so
/// the result is always at least as large as `target` and the Lanczos pass that
/// follows still does real work. On a 4000px page scaled to 1450 this decodes
/// at 2000px and cuts the resize cost by roughly 4x.
///
/// Returns None when the source is not a JPEG or no reduction applies.
fn decode_jpeg_scaled(input: &[u8], target_w: u32, target_h: u32) -> Option<DynamicImage> {
    let mut d = jpeg_decoder::Decoder::new(Cursor::new(input));
    d.read_info().ok()?;
    let info = d.info()?;
    guard_pixels(u32::from(info.width), u32::from(info.height)).ok()?;

    let tw = u16::try_from(target_w.max(1)).ok()?;
    let th = u16::try_from(target_h.max(1)).ok()?;
    let (ow, oh) = d.scale(tw, th).ok()?;
    if ow >= info.width && oh >= info.height {
        return None; // no reduction available; the normal path is faster
    }

    let px = d.decode().ok()?;
    let info = d.info()?;
    match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => {
            image::RgbImage::from_raw(u32::from(info.width), u32::from(info.height), px)
                .map(DynamicImage::ImageRgb8)
        }
        jpeg_decoder::PixelFormat::L8 => {
            image::GrayImage::from_raw(u32::from(info.width), u32::from(info.height), px)
                .map(DynamicImage::ImageLuma8)
        }
        _ => None,
    }
}

fn decode(input: &[u8]) -> Result<DynamicImage, String> {
    let reader = ImageReader::new(Cursor::new(input))
        .with_guessed_format()
        .map_err(|e| format!("unreadable image: {e}"))?;
    if let Ok((w, h)) = reader.into_dimensions() {
        guard_pixels(w, h)?;
    }
    ImageReader::new(Cursor::new(input))
        .with_guessed_format()
        .map_err(|e| format!("unreadable image: {e}"))?
        .decode()
        .map_err(|e| format!("decode failed: {e}"))
}

fn resize(img: DynamicImage, spec: ResizeSpec) -> Result<DynamicImage, String> {
    let (sw, sh) = (img.width(), img.height());
    if sw == spec.width && sh == spec.height {
        return Ok(img);
    }

    let has_alpha = img.color().has_alpha();
    let pixel_type = if has_alpha { PixelType::U8x4 } else { PixelType::U8x3 };
    let raw = if has_alpha {
        img.into_rgba8().into_raw()
    } else {
        img.into_rgb8().into_raw()
    };

    let src = FirImage::from_vec_u8(sw, sh, raw, pixel_type)
        .map_err(|e| format!("source image rejected: {e}"))?;
    let mut dst = FirImage::new(spec.width, spec.height, pixel_type);

    Resizer::new()
        .resize(
            &src,
            &mut dst,
            &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(spec.filter)),
        )
        .map_err(|e| format!("resize failed: {e}"))?;

    let buf = dst.into_vec();
    let out = if has_alpha {
        image::RgbaImage::from_raw(spec.width, spec.height, buf).map(DynamicImage::ImageRgba8)
    } else {
        image::RgbImage::from_raw(spec.width, spec.height, buf).map(DynamicImage::ImageRgb8)
    };
    out.ok_or_else(|| "resized buffer had the wrong length".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn encode_webp(img: &DynamicImage, quality: f32) -> Result<Vec<u8>, String> {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let has_alpha = img.color().has_alpha();
    let mut out_ptr: *mut u8 = std::ptr::null_mut();
    // SAFETY: buffers are w*h*channels by construction, stride matches, and the
    // allocation libwebp writes into out_ptr is freed before returning.
    let len = unsafe {
        if has_alpha {
            let rgba = img.to_rgba8();
            libwebp_sys::WebPEncodeRGBA(rgba.as_raw().as_ptr(), w, h, w * 4, quality, &mut out_ptr)
        } else {
            let rgb = img.to_rgb8();
            libwebp_sys::WebPEncodeRGB(rgb.as_raw().as_ptr(), w, h, w * 3, quality, &mut out_ptr)
        }
    };
    if len == 0 || out_ptr.is_null() {
        return Err("webp encoder produced no output".to_string());
    }
    let bytes = unsafe { std::slice::from_raw_parts(out_ptr, len) }.to_vec();
    unsafe { libwebp_sys::WebPFree(out_ptr as *mut std::ffi::c_void) };
    Ok(bytes)
}

fn encode(img: DynamicImage, output: Output) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    match output {
        Output::Jpeg(quality) => {
            let rgb = img.into_rgb8();
            JpegEncoder::new_with_quality(&mut out, quality)
                .write_image(rgb.as_raw(), rgb.width(), rgb.height(), ExtendedColorType::Rgb8)
                .map_err(|e| format!("jpeg encode failed: {e}"))?;
        }
        Output::Png(compression) => {
            let encoder = PngEncoder::new_with_quality(&mut out, compression, PngFilter::Adaptive);
            if img.color().has_alpha() {
                let p = img.into_rgba8();
                encoder
                    .write_image(p.as_raw(), p.width(), p.height(), ExtendedColorType::Rgba8)
                    .map_err(|e| format!("png encode failed: {e}"))?;
            } else {
                let p = img.into_rgb8();
                encoder
                    .write_image(p.as_raw(), p.width(), p.height(), ExtendedColorType::Rgb8)
                    .map_err(|e| format!("png encode failed: {e}"))?;
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        Output::Webp(quality) => {
            out = encode_webp(&img, quality)?;
        }
    }
    Ok(out)
}

/// The whole pipeline.
///
/// `scaled_decode` opts into the reduced-IDCT JPEG path. It is a parameter
/// rather than always-on because which decoder wins depends on the runtime:
/// `zune-jpeg` at full size is faster than `jpeg-decoder` at half size on
/// native, and the balance may differ under WebAssembly. The benchmark measures
/// both.
pub fn process(
    input: &[u8],
    resize_to: Option<ResizeSpec>,
    output: Output,
    scaled_decode: bool,
) -> Result<Vec<u8>, String> {
    let img = match (resize_to, scaled_decode) {
        (Some(spec), true) => match decode_jpeg_scaled(input, spec.width, spec.height) {
            Some(img) => img,
            None => decode(input)?,
        },
        _ => decode(input)?,
    };

    let img = match resize_to {
        Some(spec) => resize(img, spec)?,
        None => img,
    };
    encode(img, output)
}
