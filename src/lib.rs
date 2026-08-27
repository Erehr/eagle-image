//! A minimal image resizer for the Eagle CBZ reader.
//!
//! This exists because the general-purpose addon it replaces carries an AVIF
//! encoder, an SVG renderer with a font stack, a PNG optimiser and a colour
//! quantiser — roughly 20 MB of machine code, none of which a comic reader
//! calls. The plugin needs three things: read the dimensions of a page, scale
//! it down with a good filter, and write it back out as JPEG, PNG or WebP.
//!
//! The exported surface is deliberately identical to the part of
//! `@napi-rs/image` that `render-image.js` used, so it is a drop-in swap:
//! `Transformer`, `metadata()`, `fastResize()`, `jpeg()`, `png()`, `webp()`,
//! and the `FastResizeFilter` enum with its original discriminants.
//!
//! Decoding is pure Rust (zune-jpeg, png, gif, image-webp). The one C
//! dependency is libwebp, which is needed because no pure-Rust crate encodes
//! lossy WebP — `image-webp` only writes lossless VP8L, which for a
//! photographic comic page is both slower and larger than the source.

use std::io::Cursor;
use std::sync::Arc;

use fast_image_resize::images::Image as FirImage;
use fast_image_resize::{FilterType as FirFilter, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType as PngCompression, FilterType as PngFilter, PngEncoder};
use image::{DynamicImage, ExtendedColorType, ImageEncoder, ImageReader};
use napi::bindgen_prelude::*;
use napi_derive::napi;

/// Ceiling on the pixels this addon will decode, mirroring the limit the
/// plugin applies before it ever gets here.
///
/// Duplicated deliberately: an addon that will happily allocate an arbitrary
/// surface is a liability regardless of how careful its callers are, and a
/// decoded pixel count is the one thing an image header can lie about cheaply.
/// 100 MP is ~400 MB of RGBA, well above any real scan.
const MAX_PIXELS: u64 = 100_000_000;

/// Resampling filters, with the same numeric values the previous addon used so
/// the constant pinned in `render-image.js` keeps meaning Lanczos3.
#[napi]
pub enum FastResizeFilter {
    Box = 0,
    Bilinear = 1,
    Hamming = 2,
    CatmullRom = 3,
    Mitchell = 4,
    Lanczos3 = 5,
}

impl From<FastResizeFilter> for FirFilter {
    fn from(f: FastResizeFilter) -> Self {
        match f {
            FastResizeFilter::Box => FirFilter::Box,
            FastResizeFilter::Bilinear => FirFilter::Bilinear,
            FastResizeFilter::Hamming => FirFilter::Hamming,
            FastResizeFilter::CatmullRom => FirFilter::CatmullRom,
            FastResizeFilter::Mitchell => FirFilter::Mitchell,
            FastResizeFilter::Lanczos3 => FirFilter::Lanczos3,
        }
    }
}

/// PNG compression presets. `Fast` is what a display cache wants: this output
/// is regenerated on demand and thrown away, so paying for a smaller file is
/// paying for nothing.
#[napi]
pub enum CompressionType {
    Default = 0,
    Fast = 1,
    Best = 2,
}

#[napi(object)]
pub struct FastResizeOptions {
    pub width: u32,
    pub height: Option<u32>,
    pub filter: Option<FastResizeFilter>,
}

#[napi(object)]
pub struct PngEncodeOptions {
    pub compression_type: Option<u32>,
}

#[napi(object)]
pub struct Metadata {
    pub width: u32,
    pub height: u32,
    pub format: String,
}

#[derive(Clone, Copy)]
struct ResizeSpec {
    width: u32,
    height: u32,
    filter: FirFilter,
}

#[derive(Clone, Copy)]
enum Output {
    Jpeg(u8),
    Png(PngCompression),
    Webp(f32),
}

// ── Decode / resize / encode ─────────────────────────────────────────────

fn decode(input: &[u8]) -> Result<DynamicImage> {
    let reader = ImageReader::new(Cursor::new(input))
        .with_guessed_format()
        .map_err(|e| Error::from_reason(format!("unreadable image: {e}")))?;

    // Checked from the header, before the decoder allocates anything.
    if let Ok((w, h)) = reader.into_dimensions() {
        let pixels = u64::from(w) * u64::from(h);
        if pixels > MAX_PIXELS {
            return Err(Error::from_reason(format!(
                "image is {w}x{h} ({pixels} pixels), over the {MAX_PIXELS} pixel limit"
            )));
        }
    }

    let reader = ImageReader::new(Cursor::new(input))
        .with_guessed_format()
        .map_err(|e| Error::from_reason(format!("unreadable image: {e}")))?;
    reader
        .decode()
        .map_err(|e| Error::from_reason(format!("decode failed: {e}")))
}

/// Scale with fast_image_resize.
///
/// Alpha handling is left to `ResizeOptions::use_alpha`, on by default, which
/// premultiplies before convolving and divides afterwards. Without it a
/// filtered edge pulls colour out of fully transparent pixels and haloes.
fn resize(img: DynamicImage, spec: ResizeSpec) -> Result<DynamicImage> {
    let (src_w, src_h) = (img.width(), img.height());
    if src_w == spec.width && src_h == spec.height {
        return Ok(img);
    }

    let has_alpha = img.color().has_alpha();
    let pixel_type = if has_alpha { PixelType::U8x4 } else { PixelType::U8x3 };
    let raw = if has_alpha {
        img.into_rgba8().into_raw()
    } else {
        img.into_rgb8().into_raw()
    };

    let src = FirImage::from_vec_u8(src_w, src_h, raw, pixel_type)
        .map_err(|e| Error::from_reason(format!("source image rejected: {e}")))?;
    let mut dst = FirImage::new(spec.width, spec.height, pixel_type);

    Resizer::new()
        .resize(
            &src,
            &mut dst,
            &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(spec.filter)),
        )
        .map_err(|e| Error::from_reason(format!("resize failed: {e}")))?;

    let buf = dst.into_vec();
    let out = if has_alpha {
        image::RgbaImage::from_raw(spec.width, spec.height, buf).map(DynamicImage::ImageRgba8)
    } else {
        image::RgbImage::from_raw(spec.width, spec.height, buf).map(DynamicImage::ImageRgb8)
    };
    out.ok_or_else(|| Error::from_reason("resized buffer had the wrong length"))
}

fn encode_webp(img: &DynamicImage, quality: f32) -> Result<Vec<u8>> {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let has_alpha = img.color().has_alpha();

    let mut out_ptr: *mut u8 = std::ptr::null_mut();
    // SAFETY: the buffers below are sized w*h*channels by construction, the
    // stride matches, and libwebp writes a fresh allocation into out_ptr which
    // is freed through WebPFree before this function returns.
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
        return Err(Error::from_reason("webp encoder produced no output"));
    }
    let bytes = unsafe { std::slice::from_raw_parts(out_ptr, len) }.to_vec();
    unsafe { libwebp_sys::WebPFree(out_ptr as *mut std::ffi::c_void) };
    Ok(bytes)
}

fn encode(img: DynamicImage, output: Output) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    match output {
        Output::Jpeg(quality) => {
            // JPEG has no alpha; flattening to RGB avoids the encoder guessing.
            let rgb = img.into_rgb8();
            JpegEncoder::new_with_quality(&mut out, quality)
                .write_image(rgb.as_raw(), rgb.width(), rgb.height(), ExtendedColorType::Rgb8)
                .map_err(|e| Error::from_reason(format!("jpeg encode failed: {e}")))?;
        }
        Output::Png(compression) => {
            // Adaptive picks the best row filter per scanline. Measured against
            // NoFilter and Sub on a 1450px comic page it was the fastest of the
            // three *and* produced the smallest file, so there is no tradeoff to
            // make here.
            let encoder = PngEncoder::new_with_quality(&mut out, compression, PngFilter::Adaptive);
            let (buf, color) = if img.color().has_alpha() {
                let rgba = img.into_rgba8();
                (
                    (rgba.width(), rgba.height(), rgba.into_raw()),
                    ExtendedColorType::Rgba8,
                )
            } else {
                let rgb = img.into_rgb8();
                (
                    (rgb.width(), rgb.height(), rgb.into_raw()),
                    ExtendedColorType::Rgb8,
                )
            };
            let (w, h, data) = buf;
            encoder
                .write_image(&data, w, h, color)
                .map_err(|e| Error::from_reason(format!("png encode failed: {e}")))?;
        }
        Output::Webp(quality) => {
            out = encode_webp(&img, quality)?;
        }
    }
    Ok(out)
}

// ── Async tasks ──────────────────────────────────────────────────────────

/// Decode, optional resize, encode — all on the libuv thread pool, so the
/// renderer thread that paints the page never blocks on it.
pub struct EncodeTask {
    input: Arc<Vec<u8>>,
    resize: Option<ResizeSpec>,
    output: Output,
}

impl Task for EncodeTask {
    type Output = Vec<u8>;
    type JsValue = Buffer;

    fn compute(&mut self) -> Result<Self::Output> {
        let mut img = decode(&self.input)?;
        if let Some(spec) = self.resize {
            img = resize(img, spec)?;
        }
        encode(img, self.output)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output.into())
    }
}

/// Header-only dimension read. No decoder is constructed.
pub struct MetadataTask {
    input: Arc<Vec<u8>>,
}

impl Task for MetadataTask {
    type Output = Metadata;
    type JsValue = Metadata;

    fn compute(&mut self) -> Result<Self::Output> {
        let reader = ImageReader::new(Cursor::new(self.input.as_slice()))
            .with_guessed_format()
            .map_err(|e| Error::from_reason(format!("unreadable image: {e}")))?;
        let format = reader
            .format()
            .map(|f| format!("{f:?}").to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());
        let (width, height) = reader
            .into_dimensions()
            .map_err(|e| Error::from_reason(format!("no dimensions in header: {e}")))?;
        Ok(Metadata { width, height, format })
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

// ── Public API ───────────────────────────────────────────────────────────

#[napi]
pub struct Transformer {
    input: Arc<Vec<u8>>,
    resize: Option<ResizeSpec>,
}

#[napi]
impl Transformer {
    #[napi(constructor)]
    pub fn new(input: Buffer) -> Self {
        Self {
            input: Arc::new(input.to_vec()),
            resize: None,
        }
    }

    /// Dimensions and container, read from the header.
    ///
    /// The parameter exists only so the previous addon's `metadata(false)` call
    /// site keeps working; EXIF is not read.
    #[napi]
    pub fn metadata(&self, _with_exif: Option<bool>) -> AsyncTask<MetadataTask> {
        AsyncTask::new(MetadataTask { input: self.input.clone() })
    }

    /// Queue a resize. Nothing happens until an encoder is called, so the
    /// decode and the scale share one pass on the thread pool.
    #[napi]
    pub fn fast_resize(&mut self, options: FastResizeOptions) {
        let width = options.width.max(1);
        let height = options.height.unwrap_or(0).max(1);
        self.resize = Some(ResizeSpec {
            width,
            height,
            filter: options.filter.unwrap_or(FastResizeFilter::Lanczos3).into(),
        });
    }

    #[napi]
    pub fn jpeg(&self, quality: Option<u32>) -> AsyncTask<EncodeTask> {
        let q = quality.unwrap_or(90).clamp(1, 100) as u8;
        AsyncTask::new(EncodeTask {
            input: self.input.clone(),
            resize: self.resize,
            output: Output::Jpeg(q),
        })
    }

    #[napi]
    pub fn png(&self, options: Option<PngEncodeOptions>) -> AsyncTask<EncodeTask> {
        let compression = match options.and_then(|o| o.compression_type).unwrap_or(0) {
            1 => PngCompression::Fast,
            2 => PngCompression::Best,
            _ => PngCompression::Default,
        };
        AsyncTask::new(EncodeTask {
            input: self.input.clone(),
            resize: self.resize,
            output: Output::Png(compression),
        })
    }

    #[napi]
    pub fn webp(&self, quality: Option<u32>) -> AsyncTask<EncodeTask> {
        let q = quality.unwrap_or(90).clamp(1, 100) as f32;
        AsyncTask::new(EncodeTask {
            input: self.input.clone(),
            resize: self.resize,
            output: Output::Webp(q),
        })
    }
}
