//! Native addon surface. Deliberately identical to the part of
//! `@napi-rs/image` the plugin used, including enum discriminants.

use std::sync::Arc;

use image::codecs::png::CompressionType as PngCompression;
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::core::{self, Output, ResizeSpec};

/// Resampling filters, with the values the previous addon used so the constant
/// pinned in `render-image.js` keeps meaning Lanczos3.
#[napi]
pub enum FastResizeFilter {
    Box = 0,
    Bilinear = 1,
    Hamming = 2,
    CatmullRom = 3,
    Mitchell = 4,
    Lanczos3 = 5,
}

/// PNG compression presets. `Fast` is what a display cache wants: the output is
/// regenerated on demand and thrown away.
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

/// Decode, optional resize, encode — on the libuv thread pool, so the renderer
/// thread that paints the page never blocks on it.
pub struct EncodeTask {
    input: Arc<Vec<u8>>,
    resize: Option<ResizeSpec>,
    output: Output,
    scaled_decode: bool,
}

impl Task for EncodeTask {
    type Output = Vec<u8>;
    type JsValue = Buffer;

    fn compute(&mut self) -> Result<Self::Output> {
        core::process(&self.input, self.resize, self.output, self.scaled_decode)
            .map_err(Error::from_reason)
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
        let (width, height, format) =
            core::dimensions(&self.input).map_err(Error::from_reason)?;
        Ok(Metadata { width, height, format })
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

#[napi]
pub struct Transformer {
    input: Arc<Vec<u8>>,
    resize: Option<ResizeSpec>,
    scaled_decode: bool,
}

#[napi]
impl Transformer {
    #[napi(constructor)]
    pub fn new(input: Buffer) -> Self {
        Self {
            input: Arc::new(input.to_vec()),
            resize: None,
            scaled_decode: false,
        }
    }

    /// Dimensions and container from the header.
    ///
    /// The parameter exists so the previous addon's `metadata(false)` call site
    /// keeps working; EXIF is not read.
    #[napi]
    pub fn metadata(&self, _with_exif: Option<bool>) -> AsyncTask<MetadataTask> {
        AsyncTask::new(MetadataTask { input: self.input.clone() })
    }

    /// Queue a resize. Nothing happens until an encoder is called, so decode,
    /// scale and encode share one pass on the thread pool.
    #[napi]
    pub fn fast_resize(&mut self, options: FastResizeOptions) {
        self.resize = Some(ResizeSpec {
            width: options.width.max(1),
            height: options.height.unwrap_or(0).max(1),
            filter: core::filter_from_u8(options.filter.unwrap_or(FastResizeFilter::Lanczos3) as u8),
        });
    }

    /// Opt into reduced-IDCT JPEG decoding. Off by default: on native,
    /// `zune-jpeg` at full size beats `jpeg-decoder` at half size. Exposed so
    /// the benchmark can measure both, and so the wasm build can switch it on
    /// if the balance turns out differently there.
    #[napi]
    pub fn set_scaled_decode(&mut self, enabled: bool) {
        self.scaled_decode = enabled;
    }

    #[napi]
    pub fn jpeg(&self, quality: Option<u32>) -> AsyncTask<EncodeTask> {
        self.task(Output::Jpeg(quality.unwrap_or(90).clamp(1, 100) as u8))
    }

    #[napi]
    pub fn png(&self, options: Option<PngEncodeOptions>) -> AsyncTask<EncodeTask> {
        let compression = match options.and_then(|o| o.compression_type).unwrap_or(0) {
            1 => PngCompression::Fast,
            2 => PngCompression::Best,
            _ => PngCompression::Default,
        };
        self.task(Output::Png(compression))
    }

    #[napi]
    pub fn webp(&self, quality: Option<u32>) -> AsyncTask<EncodeTask> {
        self.task(Output::Webp(quality.unwrap_or(90).clamp(1, 100) as f32))
    }
}

impl Transformer {
    fn task(&self, output: Output) -> AsyncTask<EncodeTask> {
        AsyncTask::new(EncodeTask {
            input: self.input.clone(),
            resize: self.resize,
            output,
            scaled_decode: self.scaled_decode,
        })
    }
}
