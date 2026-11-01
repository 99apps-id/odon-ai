const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const CHROME = 'C:/Users/Iwan/AppData/Local/ms-playwright/chromium-1234/chrome-win64/chrome.exe';
const SVG_PATH = path.resolve(__dirname, '../assets/icons/app-icon.svg');
const OUT_DIR = path.resolve(__dirname, '../assets/icons');
const PUBLIC_DIR = path.resolve(__dirname, '../desktop/public');

// HTML file to render SVG centered in viewport
const htmlPath = path.join(OUT_DIR, 'render.html');
const svgContent = fs.readFileSync(SVG_PATH, 'utf-8');

fs.writeFileSync(
  htmlPath,
  `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <style>
    * { margin: 0; padding: 0; box-sizing: border-box; }
    html, body { width: 256px; height: 256px; background: transparent; overflow: hidden; display: flex; align-items: center; justify-content: center; }
    svg { width: 256px; height: 256px; }
  </style>
</head>
<body>
  ${svgContent}
</body>
</html>`
);

const png256 = path.join(OUT_DIR, 'icon-256.png');
const png64 = path.join(OUT_DIR, 'icon-64.png');
const png32 = path.join(OUT_DIR, 'icon-32.png');
const icoPath = path.join(OUT_DIR, 'icon.ico');
const publicIco = path.join(PUBLIC_DIR, 'favicon.ico');
const publicPng = path.join(PUBLIC_DIR, 'icon-256.png');

console.log('Rendering 256x256 PNG via Chromium headless...');
execSync(
  `"${CHROME}" --headless --disable-gpu --screenshot="${png256}" --window-size=256,256 --default-background-color=00000000 "file://${htmlPath.replace(/\\/g, '/')}"`
);

// Copy to public
fs.copyFileSync(png256, publicPng);

// Create ICO containing the 256x256 PNG
// ICO header: 6 bytes
// Icon directory entry: 16 bytes
// Image data: raw PNG
const pngBuffer = fs.readFileSync(png256);
const icoBuffer = Buffer.alloc(6 + 16 + pngBuffer.length);

// ICONDIR
icoBuffer.writeUInt16LE(0, 0); // reserved
icoBuffer.writeUInt16LE(1, 2); // type: 1 = ICO
icoBuffer.writeUInt16LE(1, 4); // count: 1 image

// ICONDIRENTRY
icoBuffer.writeUInt8(0, 6); // width: 0 means 256
icoBuffer.writeUInt8(0, 7); // height: 0 means 256
icoBuffer.writeUInt8(0, 8); // color count: 0 (no palette)
icoBuffer.writeUInt8(0, 9); // reserved
icoBuffer.writeUInt16LE(1, 10); // color planes
icoBuffer.writeUInt16LE(32, 12); // bits per pixel
icoBuffer.writeUInt32LE(pngBuffer.length, 14); // image size in bytes
icoBuffer.writeUInt32LE(22, 18); // offset to image data (6 + 16 = 22)

// Copy PNG payload
pngBuffer.copy(icoBuffer, 22);

fs.writeFileSync(icoPath, icoBuffer);
fs.writeFileSync(publicIco, icoBuffer);

// Clean up temporary render.html
try { fs.unlinkSync(htmlPath); } catch {}

console.log('Icon generation completed successfully:');
console.log(' - ' + icoPath);
console.log(' - ' + png256);
console.log(' - ' + publicIco);
