// Draws the Wirecatch mark (a signal wire on a teal tile) as a 1024 px PNG.
import { writeFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';

const N = 1024;
const out = process.argv[2];
const px = Buffer.alloc(N * N * 4);
const teal = [10, 107, 112];
const white = [255, 255, 255];
// Wire path from the sidebar glyph (viewBox 20x12), scaled into the tile.
const pts = [
  [1, 6],
  [7, 6],
  [9, 2],
  [11, 10],
  [13, 6],
  [19, 6],
].map(([x, y]) => [160 + (x / 20) * 704, 512 + ((y - 6) / 12) * 704 * 0.6]);
const half = 34;
const radius = 210;

function distSeg(px_, py, [ax, ay], [bx, by]) {
  const dx = bx - ax,
    dy = by - ay;
  const t = Math.max(0, Math.min(1, ((px_ - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)));
  return Math.hypot(px_ - (ax + t * dx), py - (ay + t * dy));
}

function insideTile(x, y) {
  const m = 40;
  const cx = Math.min(Math.max(x, m + radius), N - m - radius);
  const cy = Math.min(Math.max(y, m + radius), N - m - radius);
  return Math.hypot(x - cx, y - cy) <= radius;
}

for (let y = 0; y < N; y++) {
  for (let x = 0; x < N; x++) {
    const i = (y * N + x) * 4;
    if (!insideTile(x + 0.5, y + 0.5)) continue;
    let d = Infinity;
    for (let s = 0; s < pts.length - 1; s++)
      d = Math.min(d, distSeg(x + 0.5, y + 0.5, pts[s], pts[s + 1]));
    const a = Math.max(0, Math.min(1, half + 0.5 - d));
    for (let c = 0; c < 3; c++) px[i + c] = Math.round(teal[c] * (1 - a) + white[c] * a);
    px[i + 3] = 255;
  }
}

const raw = Buffer.alloc((N * 4 + 1) * N);
for (let y = 0; y < N; y++) {
  raw[y * (N * 4 + 1)] = 0;
  px.copy(raw, y * (N * 4 + 1) + 1, y * N * 4, (y + 1) * N * 4);
}
const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc = (buf) => {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const c = Buffer.alloc(4);
  c.writeUInt32BE(crc(td));
  return Buffer.concat([len, td, c]);
};
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(N, 0);
ihdr.writeUInt32BE(N, 4);
ihdr[8] = 8;
ihdr[9] = 6;
writeFileSync(
  out,
  Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]),
);
console.log('wrote', out);
