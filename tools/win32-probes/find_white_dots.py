# Locate whitish pixels inside the TM icon area of the user's screenshot.
import struct, zlib, os

SRC = r"C:\Users\Dev\Downloads\捕获.PNG"

d = open(SRC, "rb").read()
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

def px(x, y):
    i = (y * w + x) * 4
    return tuple(out[i:i+4])

# print a coarse color map of the icon area (left ~44px square)
print("icon area pixel map (W=white-ish, C=cyan-ish, .=bg, ?=other):")
for y in range(36):
    line = ""
    for x in range(44):
        r, g, b, a = px(x, y)
        if r > 200 and g > 200 and b > 200:
            ch = "W"
        elif b > 150 and g > 120 and r < 120:
            ch = "C"
        elif abs(r - 204) < 30 and abs(g - 228) < 30 and abs(b - 245) < 25:
            ch = "."  # TM row background (light blue)
        else:
            ch = "?"
        line += ch
    print(f"{y:2d} {line}")

print("\nwhitish pixels with exact colors:")
for y in range(36):
    for x in range(44):
        r, g, b, a = px(x, y)
        if r > 200 and g > 200 and b > 200:
            print(f"  ({x:2d},{y:2d}) rgb=({r},{g},{b})")
