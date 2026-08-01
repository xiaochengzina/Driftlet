# Upscale tray-20.png x8 for side-by-side comparison with the TM screenshot.
import os, struct, zlib

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "out")
SRC = os.path.join(HERE, "..", "..", "src-tauri", "icons", "tray-20.png")

def png_decode_rgba(path):
    d = open(path, "rb").read()
    pos, idat, w, h = 8, b"", 0, 0
    while pos < len(d):
        ln = struct.unpack(">I", d[pos:pos+4])[0]
        typ = d[pos+4:pos+8]
        if typ == b"IHDR":
            w, h = struct.unpack(">II", d[pos+8:pos+16])
        elif typ == b"IDAT":
            idat += d[pos+8:pos+8+ln]
        pos += 12 + ln
    raw = zlib.decompress(idat)
    stride = w * 4
    out = bytearray(w * h * 4)
    prev = bytearray(stride)
    p = 0
    for y in range(h):
        f = raw[p]; p += 1
        row = bytearray(raw[p:p+stride]); p += stride
        for x in range(stride):
            a = row[x-4] if x >= 4 else 0
            b = prev[x]
            c = prev[x-4] if x >= 4 else 0
            if f == 1: row[x] = (row[x] + a) & 0xFF
            elif f == 2: row[x] = (row[x] + b) & 0xFF
            elif f == 3: row[x] = (row[x] + (a + b) // 2) & 0xFF
            elif f == 4:
                pp = a + b - c
                pa, pb, pc = abs(pp - a), abs(pp - b), abs(pp - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                row[x] = (row[x] + pr) & 0xFF
        out[y*stride:(y+1)*stride] = row
        prev = row
    return w, h, bytes(out)

def png_write_rgba(path, w, h, rgba):
    raw = b"".join(b"\x00" + rgba[y*w*4:(y+1)*w*4] for y in range(h))
    def chunk(t, payload):
        c = t + payload
        return struct.pack(">I", len(payload)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) +
                           chunk(b"IDAT", zlib.compress(bytes(raw))) + chunk(b"IEND", b""))

w, h, rgba = png_decode_rgba(SRC)
S = 8
up = bytearray()
for y in range(h * S):
    for x in range(w * S):
        si = ((y // S) * w + (x // S)) * 4
        up += rgba[si:si+4]
png_write_rgba(os.path.join(OUT, "tray20_src_x8.png"), w * S, h * S, bytes(up))
print("wrote tray20_src_x8.png")

# alpha map: # = opaque, + = 128..254, . = 1..127, space = 0
for y in range(h):
    row = ""
    for x in range(w):
        a = rgba[(y * w + x) * 4 + 3]
        row += "#" if a == 255 else ("+" if a >= 128 else ("." if a > 0 else " "))
    print(f"{y:2d} {row}")

# pixels whose RGB is white-ish while alpha is partial (potential "white dot" sources)
print("\npartial-alpha pixels (x y a r g b):")
for y in range(h):
    for x in range(w):
        i = (y * w + x) * 4
        r, g, b, a = rgba[i:i+4]
        if 0 < a < 255:
            print(f"  {x:2d} {y:2d} a={a:3d} rgb=({r:3d},{g:3d},{b:3d})")
