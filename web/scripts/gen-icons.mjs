import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const iconsDir = path.resolve(__dirname, '..', 'public', 'icons');
fs.mkdirSync(iconsDir, { recursive: true });

const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128">
  <rect width="128" height="128" rx="18" fill="#1b4965"/>
  <rect x="10" y="10" width="108" height="108" rx="12" fill="#f0f0f0" stroke="#7ea8be" stroke-width="3"/>
  <!-- IC Pins -->
  <g fill="#5b7083">
    <rect x="28" y="4" width="6" height="10"/><rect x="44" y="4" width="6" height="10"/>
    <rect x="61" y="4" width="6" height="10"/><rect x="78" y="4" width="6" height="10"/>
    <rect x="94" y="4" width="6" height="10"/>
    <rect x="28" y="114" width="6" height="10"/><rect x="44" y="114" width="6" height="10"/>
    <rect x="61" y="114" width="6" height="10"/><rect x="78" y="114" width="6" height="10"/>
    <rect x="94" y="114" width="6" height="10"/>
  </g>
  <rect x="22" y="22" width="84" height="84" rx="8" fill="#173a5e"/>
  <rect x="28" y="28" width="72" height="72" rx="4" fill="#1d4e78" stroke="#5fa8d3" stroke-width="2"/>
  <text x="64" y="75" text-anchor="middle" font-family="Segoe UI, Arial, sans-serif" font-weight="700" font-size="38" fill="#ffffff">µV</text>
  <circle cx="90" cy="38" r="7" fill="#32a852" stroke="#ffffff" stroke-width="1.5"/>
</svg>
`;
fs.writeFileSync(path.join(iconsDir, 'icon.svg'), svg, 'utf8');

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) {
    c ^= buf[i];
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return (c ^ 0xffffffff) >>> 0;
}

function pngChunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const t = Buffer.from(type, 'ascii');
  const crcBuf = Buffer.concat([t, data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(crcBuf), 0);
  return Buffer.concat([len, crcBuf, crc]);
}

function makePng(size) {
  const raw = Buffer.alloc((1 + size * 4) * size);
  const setPx = (x, y, r, g, b, a = 255) => {
    if (x < 0 || y < 0 || x >= size || y >= size) return;
    const idx = y * (1 + size * 4) + 1 + x * 4;
    raw[idx] = r;
    raw[idx + 1] = g;
    raw[idx + 2] = b;
    raw[idx + 3] = a;
  };
  const fillRect = (x0, y0, w, h, col) => {
    for (let y = Math.floor(y0); y < Math.ceil(y0 + h); y++) {
      for (let x = Math.floor(x0); x < Math.ceil(x0 + w); x++) {
        setPx(x, y, col[0], col[1], col[2], 255);
      }
    }
  };
  const S = size / 128;
  fillRect(0, 0, size, size, [27, 73, 101]);
  fillRect(10 * S, 10 * S, 108 * S, 108 * S, [240, 240, 240]);
  // pins
  for (const px of [28, 44, 61, 78, 94]) {
    fillRect(px * S, 3 * S, 6 * S, 11 * S, [91, 112, 131]);
    fillRect(px * S, 114 * S, 6 * S, 11 * S, [91, 112, 131]);
  }
  fillRect(22 * S, 22 * S, 84 * S, 84 * S, [23, 58, 94]);
  fillRect(27 * S, 27 * S, 74 * S, 74 * S, [29, 78, 120]);
  // Crisp geometric 'uV' glyph
  // 'u'
  fillRect(36 * S, 50 * S, 7 * S, 26 * S, [255, 255, 255]);
  fillRect(51 * S, 50 * S, 7 * S, 26 * S, [255, 255, 255]);
  fillRect(36 * S, 70 * S, 22 * S, 6 * S, [255, 255, 255]);
  fillRect(36 * S, 76 * S, 7 * S, 10 * S, [159, 211, 242]); // mu tail
  // 'V'
  for (let y = 0; y < 32 * S; y++) {
    const t = y / (32 * S);
    const lx = (64 + t * 11) * S;
    const rx = (89 - t * 11) * S;
    fillRect(lx, 44 * S + y, 7 * S, 2, [255, 255, 255]);
    fillRect(rx - 7 * S, 44 * S + y, 7 * S, 2, [255, 255, 255]);
  }
  // green active LED corner
  fillRect(83 * S, 32 * S, 12 * S, 12 * S, [50, 168, 82]);

  for (let y = 0; y < size; y++) {
    raw[y * (1 + size * 4)] = 0;
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  const idat = zlib.deflateSync(raw, { level: 9 });
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    pngChunk('IHDR', ihdr),
    pngChunk('IDAT', idat),
    pngChunk('IEND', Buffer.alloc(0)),
  ]);
}

fs.writeFileSync(path.join(iconsDir, 'icon-192.png'), makePng(192));
fs.writeFileSync(path.join(iconsDir, 'icon-512.png'), makePng(512));
console.log('Generated SVG and 192x192 / 512x512 PNG PWA icons.');
