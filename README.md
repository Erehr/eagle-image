# eagle-image

A minimal decode / resize / encode addon for the Eagle CBZ Reader plugin.

It replaces `@napi-rs/image`, which is an excellent general-purpose library and
badly oversized for this one job. The plugin calls five things — `Transformer`,
`metadata`, `fastResize`, and three encoders — while the upstream binary also
carries an AVIF encoder, an SVG renderer with a font stack, a PNG optimiser, a
colour quantiser, a HEIC bridge and eight extra codecs.

|                          | upstream | this build |
| ------------------------ | -------- | ---------- |
| Windows x64 binary       | 20.6 MB  | ~3.3 MB    |
| plugin total             | 48 MB    | ~9 MB      |

Targets are Windows x64 and macOS (x64 + arm64). Eagle ships no Linux build, so
none is released; CI still compiles one, but only as the native half of the
WebAssembly benchmark.

## API

Identical to the subset the plugin used, including the enum values, so
`render-image.js` needs only its `require` path changed:

```js
const lib = require('eagle-image');

const t = new lib.Transformer(bytes);
const { width, height } = await t.metadata(false);
t.fastResize({ width: 1450, height: 2054, filter: lib.FastResizeFilter.Lanczos3 });
const out = await t.jpeg(85);          // or .png({compressionType: 1}) or .webp(90)
```

`fastResize` queues the resize; decode, scale and encode then happen in one pass
on the libuv thread pool, so the thread that paints the page never blocks.

## Measured against upstream

2400x3400 page scaled to 1450px wide, Lanczos3, separate processes, median of 15:

| op       | upstream | this build | output size          |
| -------- | -------- | ---------- | -------------------- |
| jpeg     | 304 ms   | 214 ms     | 862K -> 863K         |
| png      | 78 ms    | 92 ms      | 11637K -> 6118K      |
| webp     | 869 ms   | 865 ms     | 990K -> 988K         |
| metadata | 165 ms   | 2.9 ms     | —                    |

`metadata` is the large one: upstream decodes the whole image to answer it, this
build reads the header. PNG is slower per call but writes a file 47% smaller,
because upstream promotes every image to RGBA while this keeps RGB sources in
RGB.

Output is not bit-identical for the same reason. On smooth content 95.5% of
channels match exactly and the worst differs by 2/255; on adversarial content
(1px halftone dots, hard panel borders) the mean difference is 0.84/255 and
0.28% of channels differ by more than 8/255. Neither result is more correct —
it is rounding in the alpha path.

## Two backends

The work lives in `src/core.rs`, which knows nothing about bindings. Two thin
layers sit on top, chosen by target:

- **native** — napi-rs, the surface described above
- **wasm32** — wasm-bindgen, a build containing no PE or Mach-O executable and
  therefore nothing an antivirus PE heuristic scans

Both call the same `core::process`, so a benchmark between them measures the
runtime rather than two implementations that happen to differ. The `wasm` CI job
builds with `-C target-feature=+simd128` — without it, `fast_image_resize`'s
hand-written wasm kernels compile out to scalar and any measurement is
meaningless — then runs `scripts/wasm-bench.js` to compare the two on one
machine.

The wasm build has no WebP encoder: libwebp is C and does not cross to
`wasm32-unknown-unknown`, and the only pure-Rust WebP encoder writes lossless
VP8L, which on a photographic page is slower and larger than the input. WebP
pages encode as JPEG there, exactly as AVIF already does.

## Reduced-IDCT JPEG decoding

`Transformer.setScaledDecode(true)` asks `jpeg-decoder` to decode straight to
1/2, 1/4 or 1/8 of the source rather than decoding in full and discarding most
of it. On a 4000x5600 page scaled to 1450px this decodes at 2000x2800 and cuts
the resize from 87ms to 19.5ms.

It is off by default because on native it roughly breaks even: `jpeg-decoder` is
about 2x slower than `zune-jpeg`, so half-scale decoding only claws back to par
for a net ~18% off the whole pipeline. Under WebAssembly the balance may differ,
which is why it is a switch and why the benchmark measures both.

Measured on one 4000x5600 q90 page, 2.8 GHz Xeon:

```
full decode (zune-jpeg)          213.4 ms   55%
Lanczos3 resize from full res     87.3 ms   22%
jpeg encode at q85                86.5 ms   22%
```

Decode dominates. That is the number to watch when judging any backend change.

## Formats

Decodes JPEG, PNG, GIF, BMP and WebP — everything the plugin lists as a page
format except AVIF, which is served at native resolution instead, exactly as
animated pages already are. Encodes JPEG, PNG and WebP.

Every decoder is pure Rust (zune-jpeg, png, gif, image-webp). The single C
dependency is libwebp, because no pure-Rust crate encodes lossy WebP —
`image-webp` writes only lossless VP8L, which on a photographic page is both
slower and larger than the input.

## Safety

- No `panic = "abort"`. napi-rs turns a Rust panic into a thrown JS `Error` via
  `catch_unwind`, which needs unwinding; aborting would take Eagle's renderer
  process down over one bad page.
- A decoded-pixel ceiling (100 MP) is enforced from the header before any
  decoder allocates, so an image bomb is refused rather than expanded.
- No filesystem access, no network, no process spawning. Bytes in, bytes out.
- The Windows build carries a `VERSIONINFO` resource (company, product,
  description, version). Generic AV heuristics weight "unsigned PE with no
  version metadata" heavily, because that is what a dropper looks like and what
  almost no legitimately shipped DLL looks like. Rust cdylibs get none by
  default. It is not a signature and guarantees nothing.

## Building

Binaries are built on native runners by `.github/workflows/build.yml` — one job
per target, no cross-compilation. Push to `main` (or run the workflow manually)
and download the `eagle-image-package` artifact: it is the finished folder,
ready to drop into the plugin's `node_modules/`.

Locally, for the host platform only:

```sh
cargo build --release
cp target/release/libeagle_image.so eagle-image.linux-x64-gnu.node   # or .dll / .dylib
node scripts/smoke.js eagle-image.linux-x64-gnu.node
```

The WebAssembly build, which needs the `wasm32-unknown-unknown` target and
`wasm-pack`:

```sh
RUSTFLAGS='-C target-feature=+simd128' wasm-pack build --release --target nodejs --out-dir pkg
node scripts/wasm-bench.js eagle-image.linux-x64-gnu.node pkg/eagle_image.js
```

`examples/profile.rs` breaks one page down into decode, resize and encode:

```sh
cargo run --release --example profile
```
