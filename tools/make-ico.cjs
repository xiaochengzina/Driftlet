// Build a multi-size .ico from PNG files (PNG-compressed entries, Vista+).
// Usage: node make-ico.js <out.ico> <png1> <png2> ...
const fs = require("fs");

const [out, ...inputs] = process.argv.slice(2);
if (!out || inputs.length === 0) {
  console.error("usage: node make-ico.js <out.ico> <png...>");
  process.exit(1);
}

const entries = inputs.map((p) => {
  const data = fs.readFileSync(p);
  // PNG IHDR: bytes 16-19 = width, 20-23 = height (big-endian)
  const w = data.readUInt32BE(16);
  const h = data.readUInt32BE(20);
  return { w, h, data };
});

const header = Buffer.alloc(6);
header.writeUInt16LE(0, 0); // reserved
header.writeUInt16LE(1, 2); // type: icon
header.writeUInt16LE(entries.length, 4);

const dirSize = 16 * entries.length;
let offset = 6 + dirSize;
const dirs = entries.map(({ w, h, data }) => {
  const dir = Buffer.alloc(16);
  dir.writeUInt8(w >= 256 ? 0 : w, 0);
  dir.writeUInt8(h >= 256 ? 0 : h, 1);
  dir.writeUInt8(0, 2); // palette
  dir.writeUInt8(0, 3); // reserved
  dir.writeUInt16LE(1, 4); // color planes
  dir.writeUInt16LE(32, 6); // bits per pixel
  dir.writeUInt32LE(data.length, 8);
  dir.writeUInt32LE(offset, 12);
  offset += data.length;
  return dir;
});

fs.writeFileSync(out, Buffer.concat([header, ...dirs, ...entries.map((e) => e.data)]));
console.log(`OK: ${out} <- [${entries.map((e) => `${e.w}x${e.h}`).join(", ")}]`);
