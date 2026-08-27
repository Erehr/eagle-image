// One library, one process. Run from bench-all.js so neither library's
// allocator churn or pending GC can distort the other's numbers.
const fs = require('fs');
const which = process.argv[2];
const lib = which === 'upstream'
    ? require('@napi-rs/image')
    : require('../eagle-image.linux-x64-gnu.node');

const RUNS = 15, TARGET_W = 1450;

async function once(bytes, encoder, quality, srcW, srcH) {
    const t = new lib.Transformer(bytes);
    const w = Math.min(TARGET_W, srcW);
    const h = Math.max(1, Math.round(srcH * (w / srcW)));
    t.fastResize({ width: w, height: h, filter: lib.FastResizeFilter.Lanczos3 });
    if (encoder === 'png') return t.png({ compressionType: 1 });
    if (encoder === 'webp') return t.webp(quality);
    return t.jpeg(quality);
}

(async () => {
    const out = {};
    for (const [file, encoder, quality] of [['page.jpg','jpeg',85],['page.png','png',1],['page.webp','webp',90]]) {
        const bytes = fs.readFileSync(file);
        const meta = await new lib.Transformer(bytes).metadata(false);
        await once(bytes, encoder, quality, meta.width, meta.height);
        const ts = []; let res;
        for (let i = 0; i < RUNS; i++) {
            const t0 = process.hrtime.bigint();
            res = await once(bytes, encoder, quality, meta.width, meta.height);
            ts.push(Number(process.hrtime.bigint() - t0) / 1e6);
        }
        ts.sort((a, b) => a - b);
        out[encoder] = { median: ts[Math.floor(RUNS/2)], min: ts[0], bytes: res.length };
    }
    const bytes = fs.readFileSync('page.jpg');
    const mt = [];
    for (let i = 0; i < 60; i++) {
        const t0 = process.hrtime.bigint();
        await new lib.Transformer(bytes).metadata(false);
        mt.push(Number(process.hrtime.bigint() - t0) / 1e6);
    }
    mt.sort((a,b)=>a-b);
    out.metadata = { median: mt[30], min: mt[0] };
    console.log(JSON.stringify(out));
})();
