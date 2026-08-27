/**
 * Platform loader for the Eagle CBZ reader's image addon.
 *
 * Deliberately explicit rather than generated: four entries, one lookup, no
 * optionalDependencies and no npm install step. The binaries sit next to this
 * file, which is what lets the plugin vendor the whole thing as a folder.
 */
const { existsSync } = require('fs');
const { join } = require('path');

const BINARIES = {
    'win32-x64': 'eagle-image.win32-x64-msvc.node',
    'darwin-x64': 'eagle-image.darwin-x64.node',
    'darwin-arm64': 'eagle-image.darwin-arm64.node',
    'linux-x64': 'eagle-image.linux-x64-gnu.node',
};

const key = `${process.platform}-${process.arch}`;
const name = BINARIES[key];

if (!name) {
    throw new Error(`eagle-image: no binary for ${key}`);
}

const binary = join(__dirname, name);
if (!existsSync(binary)) {
    throw new Error(`eagle-image: ${name} is missing from ${__dirname}`);
}

module.exports = require(binary);
