# eagle-image

A minimal decode / resize / encode addon for the Eagle CBZ Reader plugin.

It replaces `@napi-rs/image`, which is an excellent general-purpose library and
badly oversized for this one job. The plugin calls five things — `Transformer`,
`metadata`, `fastResize`, and three encoders — while the upstream binary also
carries an AVIF encoder, an SVG renderer with a font stack, a PNG optimiser, a
colour quantiser, a HEIC bridge and eight extra codecs.

|                          | upstream | this build |
| ------------------------ | -------- | ---------- |
| Windows x64 binary       | 20.6 MB  | ~3 MB      |
| plugin total             | 48 MB    | ~9 MB      |

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
