//! A minimal image resizer for the Eagle CBZ reader.
//!
//! The general-purpose addon this replaces carries an AVIF encoder, an SVG
//! renderer with a font stack, a PNG optimiser, a colour quantiser and a HEIC
//! bridge — around 20 MB of machine code, none of which a comic reader calls.
//! The plugin needs three things: read a page's dimensions, scale it down with
//! a good filter, and write it back out.
//!
//! `core` holds the actual work and knows nothing about bindings. Two thin
//! layers sit on top of it, chosen by target:
//!
//!   native  -> napi-rs, exposing the same surface `@napi-rs/image` did, so
//!              `render-image.js` needs only its require path changed
//!   wasm32  -> wasm-bindgen, for a build that contains no PE or Mach-O
//!              executable and therefore nothing an antivirus heuristic scans
//!
//! Both call the same `core::process`, so a benchmark between them measures the
//! runtime rather than two implementations that happen to differ.

pub mod core;

#[cfg(not(target_arch = "wasm32"))]
mod napi_api;
#[cfg(not(target_arch = "wasm32"))]
pub use napi_api::*;

#[cfg(target_arch = "wasm32")]
mod wasm_api;
#[cfg(target_arch = "wasm32")]
pub use wasm_api::*;
