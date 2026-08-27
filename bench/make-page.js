// A synthetic comic page with realistic structure: gradients, hard black line
// art, halftone dots and text-like noise. Flat colour would make both encoders
// look artificially good and the resampler's work artificially cheap.
const { Transformer } = require('@napi-rs/image');
const fs = require('fs');
const W = 2400, H = 3400;
const rgb = Buffer.alloc(W * H * 3);
for (let y = 0; y < H; y++) {
  for (let x = 0; x < W; x++) {
    const i = (y * W + x) * 3;
    const panel = (x % 800 < 20 || y % 1100 < 20) ? 0 : 255;         // panel borders
    const halftone = ((x >> 1) % 7 === 0 && (y >> 1) % 7 === 0) ? 40 : 0;
    const grad = ((x / W) * 90) | 0;
    const speckle = ((x * 2654435761 ^ y * 40503) % 23) < 3 ? 60 : 0; // text-ish
    const v = Math.max(0, Math.min(255, panel - halftone - speckle - grad + (y % 3)));
    rgb[i] = v; rgb[i + 1] = v; rgb[i + 2] = Math.min(255, v + 12);
  }
}
(async () => {
  const t = new Transformer(rgb, { width: W, height: H, colorType: 'rgb8' } );
  // raw-pixel entry differs across versions; fall back to writing a PPM and letting
  // the encoder read it, which both libraries support identically.
  const ppm = Buffer.concat([Buffer.from(`P6\n${W} ${H}\n255\n`), rgb]);
  fs.writeFileSync('page.ppm', ppm);
  const out = await new Transformer(ppm).jpeg(92);
  fs.writeFileSync('page.jpg', out);
  const png = await new Transformer(ppm).png({ compressionType: 1 });
  fs.writeFileSync('page.png', png);
  const wp = await new Transformer(ppm).webp(90);
  fs.writeFileSync('page.webp', wp);
  for (const f of ['page.jpg','page.png','page.webp']) console.log(f, fs.statSync(f).size, 'bytes');
})().catch(e => { console.error('ERR', e.message); process.exit(1); });
