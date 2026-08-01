# Crop the Task Manager icon area out of the user's screenshot and upscale x8.
import struct, zlib, os

SRC = r"C:\Users\Dev\Downloads\捕获.PNG"
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out", "tm_garble_zoom.png")

d = open(SRC, "rb").read()
assert d[:8] == b"\x89PNG\r\n\x1a\n"
pos = 8
idat = b""
plte = None
trns = None
w = h = bd = ct = None
while pos < len(d):
    ln = struct.unpack(">I", d[pos:pos+4])[0]
    typ = d[pos+4:pos+8]
    payload = d[pos+8:pos+8+ln]
    if typ == b"IHDR":
        w, h = struct.unpack(">II", payload[:8]); bd, ct = payload[8], payload[9]
    elif typ == b"IDAT":
        idat += payload
    elif typ == b"PLTE":
        plte = payload
    elif typ == b"tRNS":
        trns = payload
    pos += 12 + ln

print("screenshot:", w, "x", h, "bitdepth", bd, "colortype", ct)
channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ct]
assert bd == 8
bpp = channels
stride = w * bpp
raw = zlib.decompress(idat)
out = bytearray(w * h * bpp)
prev = bytearray(stride)
p = 0
for y in range(h):
    f = raw[p]; p += 1
    row = bytearray(raw[p:p+stride]); p += stride
    for x in range(stride):
        a = row[x-bpp] if x >= bpp else 0
        b = prev[x]
        c = prev[x-bpp] if x >= bpp else 0
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

def to_rgba(x0, y0, cw, ch):
    res = bytearray()
    for y in range(y0, y0 + ch):
        for x in range(x0, x0 + cw):
            i = (y * w + x) * bpp
            if ct == 6:
                res += out[i:i+4]
            elif ct == 2:
                res += out[i:i+3] + b"\xff"
            elif ct == 3:
                idx = out[i]
                res += plte[idx*3:idx*3+3] + (bytes([trns[idx]]) if trns and idx < len(trns) else b"\xff")
    return bytes(res)

# icon occupies roughly the left square of the 124x36 row
rgba = to_rgba(0, 0, 40, 36)
S = 8
up = bytearray()
for y in range(36 * S):
    row = bytearray()
    for x in range(40 * S):
        si = ((y // S) * 40 + (x // S)) * 4
        row += rgba[si:si+4]
    up += row

def png_write(path, w, h, rgba):
    rawb = b"".join(b"\x00" + rgba[y*w*4:(y+1)*w*4] for y in range(h))
    def chunk(t, payload):
        c = t + payload
        return struct.pack(">I", len(payload)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) +
                           chunk(b"IDAT", zlib.compress(bytes(rawb))) + chunk(b"IEND", b""))

png_write(OUT, 40 * S, 36 * S, bytes(up))
print("wrote", OUT)
