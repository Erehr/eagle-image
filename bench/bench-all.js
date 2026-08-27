const { execFileSync } = require('child_process');
const run = w => JSON.parse(execFileSync('node', ['one.js', w], { encoding: 'utf8' }).trim());
const u = run('upstream'), s = run('slim');
const pad = (x, n) => String(x).padStart(n);
console.log('\n2400x3400 page -> 1450px wide, Lanczos3. Separate processes, median of 15.\n');
console.log('op          upstream        slim      delta   output size (up -> slim)');
console.log('-'.repeat(72));
for (const k of ['jpeg', 'png', 'webp']) {
    const d = (s[k].median / u[k].median - 1) * 100;
    console.log(
        k.padEnd(10) +
        pad(u[k].median.toFixed(1) + 'ms', 10) + pad(s[k].median.toFixed(1) + 'ms', 12) +
        pad((d >= 0 ? '+' : '') + d.toFixed(0) + '%', 11) + '   ' +
        (u[k].bytes/1024).toFixed(0) + 'K -> ' + (s[k].bytes/1024).toFixed(0) + 'K');
}
const dm = (s.metadata.median / u.metadata.median - 1) * 100;
console.log('metadata  ' + pad(u.metadata.median.toFixed(1) + 'ms', 10) +
    pad(s.metadata.median.toFixed(1) + 'ms', 12) + pad(dm.toFixed(0) + '%', 11));
