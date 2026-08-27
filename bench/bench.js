const fs = require('fs');
const upstream = require('@napi-rs/image');
const slim = require('../eagle-image.linux-x64-gnu.node');

const TARGET_W = 1450;           // a typical viewer width at devicePixelRatio 1
const RUNS = 8;

// Exactly what render-image.js does: construct, fastResize to the target
// width preserving aspect, encode.
async function once(lib, bytes, encoder, quality, srcW, srcH) {
    const t = new lib.Transformer(bytes);
    const targetW = Math.min(TARGET_W, srcW);
    const targetH = Math.max(1, Math.round(srcH * (targetW / srcW)));
    t.fastResize({ width: targetW, height: targetH, filter: lib.FastResizeFilter.Lanczos3 });
    if (encoder === 'png') return t.png({ compressionType: 1 });
    if (encoder === 'webp') return t.webp(quality);
    return t.jpeg(quality);
}

async function time(lib, bytes, encoder, quality, srcW, srcH) {
    await once(lib, bytes, encoder, quality, srcW, srcH);   // warm
    const ts = [];
    let out;
    for (let i = 0; i < RUNS; i++) {
        const t0 = process.hrtime.bigint();
        out = await once(lib, bytes, encoder, quality, srcW, srcH);
        ts.push(Number(process.hrtime.bigint() - t0) / 1e6);
    }
    ts.sort((a, b) => a - b);
    return { median: ts[Math.floor(ts.length / 2)], bytes: out.length };
}

(async () => {
    const cases = [
        ['page.jpg',  'jpeg', 85],
        ['page.png',  'png',   1],
        ['page.webp', 'webp', 90],
    ];

    console.log('2400x3400 page -> 1450px wide, Lanczos3, median of ' + RUNS + ' runs\n');
    console.log('input        encoder   upstream      slim     delta    output (up/slim)');
    console.log('-'.repeat(74));

    for (const [file, encoder, quality] of cases) {
        const bytes = fs.readFileSync(file);
        const meta = await new upstream.Transformer(bytes).metadata(false);
        const u = await time(upstream, bytes, encoder, quality, meta.width, meta.height);
        const s = await time(slim, bytes, encoder, quality, meta.width, meta.height);
        const delta = ((s.median / u.median - 1) * 100);
        console.log(
            file.padEnd(12) + encoder.padEnd(9) +
            (u.median.toFixed(1) + 'ms').padStart(9) +
            (s.median.toFixed(1) + 'ms').padStart(10) +
            ((delta >= 0 ? '+' : '') + delta.toFixed(0) + '%').padStart(10) + '   ' +
            (u.bytes / 1024).toFixed(0) + 'K / ' + (s.bytes / 1024).toFixed(0) + 'K'
        );
    }

    console.log('\nmetadata() header read, median of 40:');
    const bytes = fs.readFileSync('page.jpg');
    for (const [name, lib] of [['upstream', upstream], ['slim', slim]]) {
        const ts = [];
        for (let i = 0; i < 40; i++) {
            const t0 = process.hrtime.bigint();
            await new lib.Transformer(bytes).metadata(false);
            ts.push(Number(process.hrtime.bigint() - t0) / 1e6);
        }
        ts.sort((a, b) => a - b);
        console.log('  ' + name.padEnd(10) + ts[20].toFixed(2) + 'ms');
    }
})().catch(e => { console.error('ERR', e.stack); process.exit(1); });
