/**
 * Native addon vs WebAssembly, same machine, same code path, same input.
 *
 * Both backends call the identical Rust core, so what this measures is the
 * runtime cost of WebAssembly rather than two different implementations.
 *
 * The 4K fixture is generated here rather than committed: a 9 MB JPEG has no
 * business in a git repo, and building it from a JS-authored PNG keeps the
 * benchmark honest about what it is feeding in.
 */
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

const native = require(path.resolve(process.argv[2]));
const wasm = require(path.resolve(process.argv[3]));

const W = 4000, H = 5600, TARGET_W = 1450;
const TARGET_H = Math.round(H * (TARGET_W / W));
const RUNS = 7;

function crc32(b) {
    let c = 0xFFFFFFFF;
    for (let i = 0; i < b.length; i++) {
        c ^= b[i];
        for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xEDB88320 & -(c & 1));
    }
    return (c ^ 0xFFFFFFFF) >>> 0;
}

function buildPng(w, h) {
    const chunk = (type, data) => {
        const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
        const td = Buffer.concat([Buffer.from(type), data]);
        const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(td));
        return Buffer.concat([len, td, crc]);
    };
    const ihdr = Buffer.alloc(13);
    ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4);
    ihdr[8] = 8; ihdr[9] = 2;
    const stride = 1 + w * 3;
    const raw = Buffer.alloc(h * stride);
    for (let y = 0; y < h; y++) {
        const row = y * stride;
        for (let x = 0; x < w; x++) {
            const i = row + 1 + x * 3;
            // Detail at several scales, so the encoder and the resampler both
            // do realistic work rather than compressing a flat field.
            const v = 128 + 70 * Math.sin(x / 150) * Math.cos(y / 200)
                + 30 * Math.sin((x + y) / 25)
                + (((x * 2654435761) ^ (y * 40503)) % 17 < 2 ? 45 : 0);
            const c = v < 0 ? 0 : v > 255 ? 255 : v | 0;
            raw[i] = c; raw[i + 1] = (c * 0.93) | 0; raw[i + 2] = c > 240 ? 255 : (c * 1.06) | 0;
        }
    }
    return Buffer.concat([
        Buffer.from([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
        chunk('IHDR', ihdr),
        chunk('IDAT', zlib.deflateSync(raw, { level: 1 })),
        chunk('IEND', Buffer.alloc(0)),
    ]);
}

async function median(label, fn) {
    await fn();
    const ts = [];
    let out;
    for (let i = 0; i < RUNS; i++) {
        const t0 = process.hrtime.bigint();
        out = await fn();
        ts.push(Number(process.hrtime.bigint() - t0) / 1e6);
    }
    ts.sort((a, b) => a - b);
    const m = ts[Math.floor(RUNS / 2)];
    console.log(`  ${label.padEnd(46)} ${m.toFixed(1).padStart(8)} ms   ${(out.length / 1024).toFixed(0)}K`);
    return m;
}

(async () => {
    console.log('\nbuilding the 4K fixture...');
    const png = buildPng(W, H);
    const t = new native.Transformer(png);
    t.fastResize({ width: W, height: H, filter: native.FastResizeFilter.Lanczos3 });
    const jpg = await t.jpeg(90);
    console.log(`  ${W}x${H} q90 jpeg, ${(jpg.length / 1024 / 1024).toFixed(2)} MB\n`);

    console.log(`4000x5600 -> ${TARGET_W}px wide, Lanczos3, jpeg q85, median of ${RUNS}:\n`);

    const nativePlain = await median('native  (full decode)', async () => {
        const x = new native.Transformer(jpg);
        x.fastResize({ width: TARGET_W, height: TARGET_H, filter: native.FastResizeFilter.Lanczos3 });
        return x.jpeg(85);
    });
    const nativeScaled = await median('native  (reduced-IDCT decode)', async () => {
        const x = new native.Transformer(jpg);
        x.setScaledDecode(true);
        x.fastResize({ width: TARGET_W, height: TARGET_H, filter: native.FastResizeFilter.Lanczos3 });
        return x.jpeg(85);
    });
    const wasmPlain = await median('wasm    (full decode)', async () =>
        wasm.resize_encode(jpg, TARGET_W, TARGET_H, 5, 0, 85, false));
    const wasmScaled = await median('wasm    (reduced-IDCT decode)', async () =>
        wasm.resize_encode(jpg, TARGET_W, TARGET_H, 5, 0, 85, true));

    const bestNative = Math.min(nativePlain, nativeScaled);
    const bestWasm = Math.min(wasmPlain, wasmScaled);
    console.log(`\n  best native ${bestNative.toFixed(1)}ms   best wasm ${bestWasm.toFixed(1)}ms` +
        `   ->  wasm is ${(bestWasm / bestNative).toFixed(2)}x slower\n`);

    // This runner is not Rob's machine. The ratio is the portable number; the
    // absolute figures scale with whatever CPU the reader has.
    console.log('  NOTE: the ratio is what transfers between machines, not the absolute ms.');
})().catch(e => { console.error(e); process.exit(1); });
