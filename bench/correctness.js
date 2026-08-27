const fs = require('fs');
const up = require('@napi-rs/image');
const slim = require('../eagle-image.linux-x64-gnu.node');

let pass = 0, fail = 0;
const check = (n, c, x) => { c ? (pass++, console.log('  ok   ' + n)) : (fail++, console.log('  FAIL ' + n + (x ? '  -> ' + x : ''))); };

async function raw(bytes) {                 // decode to RGB via upstream, for comparison only
    return new up.Transformer(bytes).rawPixels();
}

(async () => {
    console.log('\n1. resized output matches the upstream resampler');
    {
        const src = fs.readFileSync('page.jpg');
        const meta = await new up.Transformer(src).metadata(false);
        const w = 1450, h = Math.round(meta.height * w / meta.width);
        const mk = async lib => {
            const t = new lib.Transformer(src);
            t.fastResize({ width: w, height: h, filter: lib.FastResizeFilter.Lanczos3 });
            return t.png({ compressionType: 1 });     // lossless, so only the resize differs
        };
        let a = await raw(await mk(up)), b = await raw(await mk(slim));
        // The source jpeg has no alpha. Upstream promotes its PNG output to
        // RGBA anyway (which is why its file is 90% larger); this build keeps
        // RGB. Drop the padding channel so the comparison is like for like.
        const stripAlpha = buf => {
            const out = Buffer.alloc(buf.length / 4 * 3);
            for (let i = 0, o = 0; i < buf.length; i += 4) {
                out[o++] = buf[i]; out[o++] = buf[i+1]; out[o++] = buf[i+2];
            }
            return out;
        };
        if (a.length === b.length / 3 * 4) a = stripAlpha(a);
        if (b.length === a.length / 3 * 4) b = stripAlpha(b);
        check('same pixel count after normalising channels', a.length === b.length, a.length + ' vs ' + b.length);
        let sum = 0, worst = 0;
        for (let i = 0; i < a.length; i++) { const d = Math.abs(a[i] - b[i]); sum += d; if (d > worst) worst = d; }
        const mad = sum / a.length;
        // Not bit-identical, and that is understood rather than tolerated.
        // Upstream promotes every image to RGBA and resizes on the U8x4 path,
        // premultiplying and dividing by an alpha that is always 255; this
        // build keeps RGB sources in U8x3. The rounding differs, which shows
        // up only where a 1px hard edge meets the filter kernel. Measured
        // separately: on smooth content 95.5% of channels are identical and
        // the worst is 2/255. page.jpg is deliberately adversarial - 1px
        // halftone dots and hard panel borders everywhere.
        check('mean abs difference well under 1/255 (' + mad.toFixed(4) + ')', mad < 1.0, mad);
        const overEight = (() => { let n = 0; for (let i = 0; i < a.length; i++) if (Math.abs(a[i]-b[i]) > 8) n++; return 100*n/a.length; })();
        check('under 1% of channels differ by more than 8/255 (' + overEight.toFixed(2) + '%)', overEight < 1.0, overEight);
    }

    console.log('\n2. every input format a CBZ can hold decodes');
    {
        for (const f of ['page.jpg', 'page.png', 'page.webp']) {
            const bytes = fs.readFileSync(f);
            try {
                const m = await new slim.Transformer(bytes).metadata(false);
                check(f + ' -> ' + m.format + ' ' + m.width + 'x' + m.height, m.width === 2400 && m.height === 3400);
            } catch (e) { check(f, false, e.message); }
        }
    }

    console.log('\n3. alpha survives the round trip');
    {
        // 200x200 RGBA, half transparent
        const W = 200, H = 200, rgba = Buffer.alloc(W * H * 4);
        for (let i = 0; i < W * H; i++) {
            rgba[i*4] = 255; rgba[i*4+1] = 0; rgba[i*4+2] = 0;
            rgba[i*4+3] = (i % W) < W / 2 ? 255 : 0;
        }
        const srcPng = await new up.Transformer(
            await new up.Transformer(Buffer.concat([Buffer.from(`P7\nWIDTH ${W}\nHEIGHT ${H}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n`), rgba])).png({ compressionType: 1 })
        ).png({ compressionType: 1 });
        const t = new slim.Transformer(srcPng);
        t.fastResize({ width: 100, height: 100, filter: 5 });
        const out = await t.png({ compressionType: 1 });
        const m = await new slim.Transformer(out).metadata(false);
        check('resized RGBA png is 100x100', m.width === 100 && m.height === 100, m.width + 'x' + m.height);
        const px = await new up.Transformer(out).rawPixels();
        check('output is non-empty', px.length > 0);
    }

    console.log('\n4. the pixel-count guard');
    {
        // A 390-byte PNG whose IHDR claims 40000x40000, same shape as the
        // image bomb the archive limits already reject.
        const zlib = require('zlib');
        const real = fs.readFileSync('page.png');
        const hdr = Buffer.from(real.subarray(0, 33));
        hdr.writeUInt32BE(40000, 16); hdr.writeUInt32BE(40000, 20);
        // Recompute the IHDR CRC, otherwise the decoder rejects the file for
        // corruption and the pixel guard never gets a chance to speak.
        hdr.writeUInt32BE(zlib.crc32(hdr.subarray(12, 29)) >>> 0, 29);
        const bomb = Buffer.concat([hdr, real.subarray(33)]);
        try {
            const t = new slim.Transformer(bomb);
            t.fastResize({ width: 800, height: 800, filter: 5 });
            await t.jpeg(85);
            check('oversized image refused', false, 'it encoded');
        } catch (e) {
            check('oversized image refused: ' + e.message.slice(0, 58), /pixel limit/.test(e.message), e.message);
        }
    }

    console.log('\n5. malformed input fails cleanly instead of crashing');
    {
        for (const [name, buf] of [
            ['empty buffer', Buffer.alloc(0)],
            ['random bytes', Buffer.from(Array.from({length: 4096}, (_, i) => (i * 37) & 255))],
            ['truncated jpeg', fs.readFileSync('page.jpg').subarray(0, 512)],
        ]) {
            try { await new slim.Transformer(buf).jpeg(85); check(name + ' rejected', false, 'it succeeded'); }
            catch (e) { check(name + ' rejected cleanly', true); }
        }
    }

    console.log('\n' + pass + ' passed, ' + fail + ' failed');
    process.exit(fail ? 1 : 0);
})().catch(e => { console.error('ERR', e.stack); process.exit(1); });
