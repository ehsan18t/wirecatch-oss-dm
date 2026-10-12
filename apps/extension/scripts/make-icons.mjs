// Draws the toolbar icons (the same shape as the logo in the popup) into public/icon/.
// Run with `node scripts/make-icons.mjs` after changing the design; the PNGs are committed.
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateSync } from 'node:zlib';

const SIZES = [16, 32, 48, 96, 128];
const BLUE = [0x1d, 0x4e, 0xd8];
const WHITE = [0xff, 0xff, 0xff];

// Shapes in a 24 x 24 box, as in the popup's SVG logo.
const SEGMENTS = [
  [12, 6, 12, 15],
  [8, 11, 12, 15],
  [12, 15, 16, 11],
  [7, 18, 17, 18],
];
const STROKE = 1; // half of the stroke width

function insideRoundedRect(x, y) {
  const [min, max, r] = [1, 23, 6];
  if (x < min || x > max || y < min || y > max) return false;
  const cx = Math.min(Math.max(x, min + r), max - r);
  const cy = Math.min(Math.max(y, min + r), max - r);
  return (x - cx) ** 2 + (y - cy) ** 2 <= r * r;
}

function nearSegment(x, y) {
  return SEGMENTS.some(([x1, y1, x2, y2]) => {
    const dx = x2 - x1;
    const dy = y2 - y1;
    const t = Math.max(0, Math.min(1, ((x - x1) * dx + (y - y1) * dy) / (dx * dx + dy * dy)));
    return (x - x1 - t * dx) ** 2 + (y - y1 - t * dy) ** 2 <= STROKE * STROKE;
  });
}

/** Samples per pixel along each axis, for smooth edges. */
const SAMPLES = 4;
/** The design's box: 24 x 24 units, as in the SVG logo. */
const BOX = 24;

/** How much of a pixel the rounded square covers, and how much of that the arrow covers (0 to 1 each). */
function coverage(px, py, size) {
  let rect = 0;
  let stroke = 0;
  for (let sy = 0; sy < SAMPLES; sy += 1) {
    for (let sx = 0; sx < SAMPLES; sx += 1) {
      const x = ((px + (sx + 0.5) / SAMPLES) / size) * BOX;
      const y = ((py + (sy + 0.5) / SAMPLES) / size) * BOX;
      if (!insideRoundedRect(x, y)) continue;
      rect += 1;
      if (nearSegment(x, y)) stroke += 1;
    }
  }
  return { alpha: rect / (SAMPLES * SAMPLES), white: rect === 0 ? 0 : stroke / rect };
}

function draw(size) {
  const pixels = Buffer.alloc(size * size * 4);
  for (let py = 0; py < size; py += 1) {
    for (let px = 0; px < size; px += 1) {
      const { alpha, white } = coverage(px, py, size);
      const i = (py * size + px) * 4;
      for (let c = 0; c < 3; c += 1) {
        pixels[i + c] = Math.round(BLUE[c] * (1 - white) + WHITE[c] * white);
      }
      pixels[i + 3] = Math.round(alpha * 255);
    }
  }
  return pixels;
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buffer) {
  let c = 0xffffffff;
  for (const byte of buffer) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}

function png(size, rgba) {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0);
  header.writeUInt32BE(size, 4);
  header[8] = 8; // bit depth
  header[9] = 6; // RGBA
  const rows = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y += 1) {
    rows[y * (size * 4 + 1)] = 0; // no filter
    rgba.copy(rows, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(rows, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

const out = join(dirname(fileURLToPath(import.meta.url)), '..', 'public', 'icon');
mkdirSync(out, { recursive: true });
for (const size of SIZES) {
  writeFileSync(join(out, `${size}.png`), png(size, draw(size)));
}
console.log(`Wrote ${SIZES.length} icons to ${out}`);
