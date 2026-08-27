//! Where the time actually goes on one 4K page, and what DCT-scaled decoding
//! is worth. Self-contained so it does not link the cdylib.

use std::io::Cursor;
use std::time::Instant;

use fast_image_resize::images::Image as FirImage;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, ExtendedColorType, ImageEncoder, ImageReader};

const RUNS: u32 = 7;

fn bench<T>(label: &str, mut f: impl FnMut() -> T) -> T {
    let mut best = f();
    let mut times = Vec::new();
    for _ in 0..RUNS {
        let t = Instant::now();
        best = f();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("  {:<44} {:>8.1} ms", label, times[times.len() / 2]);
    best
}

fn main() {
    let bytes = std::fs::read("bench/page4k.jpg").expect("run the node fixture step first");
    let target_w = 1450u32;

    println!("\n4000x5600 q90 jpeg -> {}px wide\n", target_w);

    println!("current pipeline:");
    let full = bench("1. full decode (image crate / zune-jpeg)", || {
        ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format().unwrap()
            .decode().unwrap()
    });
    let (sw, sh) = (full.width(), full.height());
    let target_h = (sh as f64 * (target_w as f64 / sw as f64)).round() as u32;

    let raw = full.to_rgb8().into_raw();
    let resized = bench("2. Lanczos3 resize from full res", || {
        let src = FirImage::from_vec_u8(sw, sh, raw.clone(), PixelType::U8x3).unwrap();
        let mut dst = FirImage::new(target_w, target_h, PixelType::U8x3);
        Resizer::new().resize(&src, &mut dst,
            &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))).unwrap();
        dst
    });
    let rbuf = resized.into_vec();
    bench("3. jpeg encode at q85", || {
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
            .write_image(&rbuf, target_w, target_h, ExtendedColorType::Rgb8).unwrap();
        out
    });

    println!("\nwith DCT-scaled decode (jpeg-decoder picks a smaller IDCT):");
    let scaled = bench("1b. decode straight to >= target size", || {
        let mut d = jpeg_decoder::Decoder::new(Cursor::new(&bytes));
        // Ask for the target; the decoder rounds up to 1/1, 1/2, 1/4 or 1/8.
        d.scale(target_w as u16, target_h as u16).unwrap();
        let px = d.decode().unwrap();
        let info = d.info().unwrap();
        (px, info.width as u32, info.height as u32)
    });
    let (spx, sw2, sh2) = scaled;
    println!("      decoded at {}x{} instead of {}x{}", sw2, sh2, sw, sh);

    bench("2b. Lanczos3 resize from the reduced size", || {
        let src = FirImage::from_vec_u8(sw2, sh2, spx.clone(), PixelType::U8x3).unwrap();
        let mut dst = FirImage::new(target_w, target_h, PixelType::U8x3);
        Resizer::new().resize(&src, &mut dst,
            &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))).unwrap();
        dst
    });
}
