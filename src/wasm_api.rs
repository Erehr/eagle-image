//! WebAssembly surface.
//!
//! Flat functions rather than a mirror of the native class: a JS shim rebuilds
//! the `Transformer` shape on top of these, which keeps `render-image.js`
//! identical across both backends without paying for cross-boundary object
//! state.
//!
//! WebP encoding is absent here. libwebp is C and does not cross to
//! `wasm32-unknown-unknown`, and the only pure-Rust WebP encoder writes
//! lossless VP8L, which on a photographic page is slower and larger than the
//! input. The shim routes WebP pages to JPEG, exactly as AVIF already is.

use image::codecs::png::CompressionType as PngCompression;
use wasm_bindgen::prelude::*;

use crate::core::{self, Output, ResizeSpec};

/// `[width, height]`, read from the header without constructing a decoder.
#[wasm_bindgen]
pub fn metadata(input: &[u8]) -> Result<Vec<u32>, JsError> {
    let (w, h, _) = core::dimensions(input).map_err(|e| JsError::new(&e))?;
    Ok(vec![w, h])
}

/// Container name from the header, for callers that need it.
#[wasm_bindgen]
pub fn format(input: &[u8]) -> Result<String, JsError> {
    let (_, _, f) = core::dimensions(input).map_err(|e| JsError::new(&e))?;
    Ok(f)
}

/// Decode, scale to `width` x `height`, encode.
///
/// `encoder`: 0 = jpeg, 1 = png. `filter` matches FastResizeFilter.
/// `quality` is the JPEG quality, or the PNG compression preset.
#[wasm_bindgen]
pub fn resize_encode(
    input: &[u8],
    width: u32,
    height: u32,
    filter: u8,
    encoder: u8,
    quality: u32,
    scaled_decode: bool,
) -> Result<Vec<u8>, JsError> {
    let spec = ResizeSpec {
        width: width.max(1),
        height: height.max(1),
        filter: core::filter_from_u8(filter),
    };
    let output = match encoder {
        1 => Output::Png(match quality {
            1 => PngCompression::Fast,
            2 => PngCompression::Best,
            _ => PngCompression::Default,
        }),
        _ => Output::Jpeg(quality.clamp(1, 100) as u8),
    };
    core::process(input, Some(spec), output, scaled_decode).map_err(|e| JsError::new(&e))
}
