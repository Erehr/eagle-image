/**
 * Loads the freshly built addon and pushes one image all the way through.
 *
 * Runs on each build runner, so a binary that compiles but cannot decode, or
 * that was linked against something missing on the target, fails the build
 * rather than reaching a user.
 */
const assert = require('assert');
const zlib = require('zlib');

const lib = require(require('path').resolve(process.argv[2]));

// A 64x64 PNG built here rather than committed, so the test has no fixtures.
function png(w, h) {
    const crcOf = b => { const o = Buffer.alloc(4); o.writeUInt32BE(zlib.crc32(b) >>> 0); return o; };
    const chunk = (type, data) => {
        const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
        const td = Buffer.concat([Buffer.from(type), data]);
        return Buffer.concat([len, td, crcOf(td)]);
    };
    const ihdr = Buffer.alloc(13);
    ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4);
    ihdr[8] = 8; ihdr[9] = 2;
    const raw = Buffer.alloc(h * (1 + w * 3));
    for (let y = 0; y < h; y++)
        for (let x = 0; x < w; x++) {
            const i = y * (1 + w * 3) + 1 + x * 3;
            raw[i] = (x * 4) & 255; raw[i + 1] = (y * 4) & 255; raw[i + 2] = 128;
        }
    return Buffer.concat([
        Buffer.from([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
        chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0)),
    ]);
}

(async () => {
    assert.strictEqual(lib.FastResizeFilter.Lanczos3, 5, 'Lanczos3 must stay 5');
    assert.strictEqual(lib.CompressionType.Fast, 1, 'Fast must stay 1');

    const src = png(64, 64);

    const meta = await new lib.Transformer(src).metadata(false);
    assert.strictEqual(meta.width, 64);
    assert.strictEqual(meta.height, 64);

    for (const [name, run] of [
        ['jpeg', t => t.jpeg(85)],
        ['png', t => t.png({ compressionType: 1 })],
        ['webp', t => t.webp(90)],
    ]) {
        const t = new lib.Transformer(src);
        t.fastResize({ width: 32, height: 32, filter: lib.FastResizeFilter.Lanczos3 });
        const out = await run(t);
        assert.ok(out.length > 0, name + ' produced no bytes');
        const m = await new lib.Transformer(out).metadata(false);
        assert.strictEqual(m.width, 32, name + ' width');
        assert.strictEqual(m.height, 32, name + ' height');
        console.log(`  ${name.padEnd(5)} ok  ${out.length} bytes, ${m.width}x${m.height}`);
    }

    await assert.rejects(
        () => new lib.Transformer(Buffer.from('nonsense')).jpeg(85),
        'garbage input must reject rather than crash');

    console.log(`smoke test passed on ${process.platform}-${process.arch}`);
})().catch(e => { console.error(e); process.exit(1); });
