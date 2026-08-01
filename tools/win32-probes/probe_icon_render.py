# Probe: why does Driftlet's icon garble in Task Manager / tray drag?
#
#  1. Rebuilds the tray HICON exactly the way tray-icon 0.24.1 / tao 0.35.3 do
#     (CreateIcon with a 1-BYTE-PER-PIXEL "mask" where a 1-BIT-PER-PIXEL mask is
#     expected) and renders it via DrawIconEx + dumps the AND mask.
#  2. Builds a correct HICON (CreateIconIndirect + proper 1bpp mask) for contrast.
#  3. Renders the exe's embedded icon through the extraction APIs a shell
#     component would use (SHGetFileInfo / ExtractIconEx / PrivateExtractIcons).
#
# Outputs PNGs into tools/win32-probes/out/ for eyeballing.

import ctypes, os, struct, zlib
from ctypes import wintypes as W

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "out")
os.makedirs(OUT, exist_ok=True)

ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
TRAY_PNG = os.path.join(ROOT, "src-tauri", "icons", "tray-20.png")
EXE = os.path.join(ROOT, "src-tauri", "target", "debug", "Driftlet.exe")

u32 = ctypes.windll.user32
g32 = ctypes.windll.gdi32
s32 = ctypes.windll.shell32
o32 = ctypes.windll.ole32
k32 = ctypes.windll.kernel32

for f, res in [
    (u32.CreateIcon, W.HICON), (u32.CreateIconIndirect, W.HICON),
    (u32.DrawIconEx, W.BOOL), (u32.GetIconInfo, W.BOOL),
    (u32.FillRect, ctypes.c_int), (u32.GetDC, W.HDC),
    (u32.PrivateExtractIconsW, W.UINT), (s32.ExtractIconExW, W.UINT),
    (s32.SHGetFileInfoW, W.DWORD),
    (g32.CreateCompatibleDC, W.HDC), (g32.CreateSolidBrush, W.HBRUSH),
    (g32.CreateDIBSection, W.HBITMAP), (g32.SelectObject, W.HGDIOBJ),
    (g32.GetDIBits, ctypes.c_int), (g32.GetObjectW, ctypes.c_int),
    (g32.BitBlt, W.BOOL), (g32.DeleteObject, W.BOOL), (g32.DeleteDC, W.BOOL),
]:
    f.restype = res

g32.GetObjectW.argtypes = [W.HGDIOBJ, ctypes.c_int, W.LPVOID]
g32.SelectObject.argtypes = [W.HDC, W.HGDIOBJ]
g32.BitBlt.argtypes = [W.HDC, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                       W.HDC, ctypes.c_int, ctypes.c_int, W.DWORD]
g32.DeleteObject.argtypes = [W.HGDIOBJ]
g32.CreateCompatibleDC.argtypes = [W.HDC]
g32.DeleteDC.argtypes = [W.HDC]
g32.CreateSolidBrush.argtypes = [W.COLORREF]
g32.CreateDIBSection.argtypes = [W.HDC, W.LPVOID, W.UINT, W.LPVOID, W.HANDLE, W.DWORD]
g32.CreateBitmap.argtypes = [ctypes.c_int, ctypes.c_int, W.UINT, W.UINT, W.LPVOID]
g32.CreateBitmap.restype = W.HBITMAP
g32.GetDIBits.argtypes = [W.HDC, W.HBITMAP, W.UINT, W.UINT, W.LPVOID, W.LPVOID, W.UINT]
u32.DrawIconEx.argtypes = [W.HDC, ctypes.c_int, ctypes.c_int, W.HICON,
                           ctypes.c_int, ctypes.c_int, W.UINT, W.HBRUSH, W.UINT]
u32.FillRect.argtypes = [W.HDC, W.LPVOID, W.HBRUSH]
u32.DestroyIcon.argtypes = [W.HICON]
u32.CreateIconIndirect.argtypes = [W.LPVOID]
u32.CreateIcon.argtypes = [W.HINSTANCE, ctypes.c_int, ctypes.c_int,
                           W.BYTE, W.BYTE, W.LPVOID, W.LPVOID]
u32.GetIconInfo.argtypes = [W.HICON, W.LPVOID]
u32.PrivateExtractIconsW.argtypes = [W.LPCWSTR, ctypes.c_int, ctypes.c_int,
                                     ctypes.c_int, W.LPVOID, W.LPVOID, W.UINT, W.UINT]
s32.ExtractIconExW.argtypes = [W.LPCWSTR, ctypes.c_int, W.LPVOID, W.LPVOID, W.UINT]
s32.SHGetFileInfoW.argtypes = [W.LPCWSTR, W.DWORD, W.LPVOID, W.UINT, W.UINT]

u32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
o32.CoInitializeEx(None, 0)

# ---------------- pure-python PNG (decode ct6/bd8, encode RGBA8) --------------
def png_decode_rgba(path):
    d = open(path, "rb").read()
    assert d[:8] == b"\x89PNG\r\n\x1a\n"
    pos, idat, w, h = 8, b"", 0, 0
    while pos < len(d):
        ln = struct.unpack(">I", d[pos:pos+4])[0]
        typ = d[pos+4:pos+8]
        if typ == b"IHDR":
            w, h = struct.unpack(">II", d[pos+8:pos+16])
            assert d[pos+16] == 8 and d[pos+17] == 6, "expected RGBA8"
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
    open(path, "wb").write(
        b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) +
        chunk(b"IDAT", zlib.compress(bytes(raw))) + chunk(b"IEND", b""))

# ---------------- win32 helpers ----------------------------------------------
class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [("biSize", W.DWORD), ("biWidth", W.LONG), ("biHeight", W.LONG),
                ("biPlanes", W.WORD), ("biBitCount", W.WORD), ("biCompression", W.DWORD),
                ("biSizeImage", W.DWORD), ("biXPelsPerMeter", W.LONG),
                ("biYPelsPerMeter", W.LONG), ("biClrUsed", W.DWORD), ("biClrImportant", W.DWORD)]

class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", W.DWORD * 3)]

class ICONINFO(ctypes.Structure):
    _fields_ = [("fIcon", W.BOOL), ("xHotspot", W.DWORD), ("yHotspot", W.DWORD),
                ("hbmMask", W.HBITMAP), ("hbmColor", W.HBITMAP)]

class BITMAP_(ctypes.Structure):
    _fields_ = [("bmType", W.LONG), ("bmWidth", W.LONG), ("bmHeight", W.LONG),
                ("bmWidthBytes", W.LONG), ("bmPlanes", W.WORD), ("bmBitsPixel", W.WORD),
                ("bmBits", W.LPVOID)]

def make_dib(w, h):
    bi = BITMAPINFO()
    bi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bi.bmiHeader.biWidth = w
    bi.bmiHeader.biHeight = -h  # top-down
    bi.bmiHeader.biPlanes = 1
    bi.bmiHeader.biBitCount = 32
    bi.bmiHeader.biCompression = 0  # BI_RGB
    hdc = g32.CreateCompatibleDC(None)
    bits = W.LPVOID()
    hbm = g32.CreateDIBSection(hdc, ctypes.byref(bi), 0, ctypes.byref(bits), None, 0)
    old = g32.SelectObject(hdc, hbm)
    return hdc, hbm, bits, old

def dib_to_rgba(hdc, hbm, w, h):
    bi = BITMAPINFO()
    bi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bi.bmiHeader.biWidth = w
    bi.bmiHeader.biHeight = -h
    bi.bmiHeader.biPlanes = 1
    bi.bmiHeader.biBitCount = 32
    buf = (ctypes.c_ubyte * (w * h * 4))()
    g32.GetDIBits(hdc, hbm, 0, h, buf, ctypes.byref(bi), 0)
    rgba = bytearray(w * h * 4)
    for i in range(w * h):
        b, g, r, a = buf[i*4:i*4+4]
        rgba[i*4:i*4+4] = bytes((r, g, b, a))
    return bytes(rgba)

def upscale(rgba, w, h, s):
    out = bytearray(w * s * h * s * 4)
    for y in range(h * s):
        for x in range(w * s):
            si = ((y // s) * w + (x // s)) * 4
            di = (y * w * s + x) * 4
            out[di:di+4] = rgba[si:si+4]
    return bytes(out)

def render_icon(hicon, w, h, bg, name, scale=4):
    hdc, hbm, bits, old = make_dib(w, h)
    class RECT(ctypes.Structure):
        _fields_ = [("left", W.LONG), ("top", W.LONG), ("right", W.LONG), ("bottom", W.LONG)]
    rc = RECT(0, 0, w, h)
    u32.FillRect(hdc, ctypes.byref(rc), g32.CreateSolidBrush(bg))
    u32.DrawIconEx(hdc, 0, 0, hicon, w, h, 0, None, 0x0003)  # DI_NORMAL
    rgba = dib_to_rgba(hdc, hbm, w, h)
    if scale != 1:
        rgba = upscale(rgba, w, h, scale)
        w, h = w * scale, h * scale
    png_write_rgba(os.path.join(OUT, name), w, h, rgba)
    g32.SelectObject(hdc, old); g32.DeleteObject(hbm); g32.DeleteDC(hdc)
    print("wrote", name)

def dump_mask(hicon, name):
    ii = ICONINFO()
    u32.GetIconInfo(hicon, ctypes.byref(ii))
    bm = BITMAP_()
    g32.GetObjectW(ii.hbmMask, ctypes.sizeof(BITMAP_), ctypes.byref(bm))
    print(f"  mask: {bm.bmWidth}x{bm.bmHeight} planes={bm.bmPlanes} bpp={bm.bmBitsPixel}")
    if ii.hbmColor:
        g32.GetObjectW(ii.hbmColor, ctypes.sizeof(BITMAP_), ctypes.byref(bm))
        print(f"  color: {bm.bmWidth}x{bm.bmHeight} planes={bm.bmPlanes} bpp={bm.bmBitsPixel}")
    mw, mh = bm.bmWidth, bm.bmHeight
    # paint mask bitmap into a 32bpp dib (white bg, mask comes out black)
    hdc, hbm, bits, old = make_dib(mw, mh)
    class RECT(ctypes.Structure):
        _fields_ = [("left", W.LONG), ("top", W.LONG), ("right", W.LONG), ("bottom", W.LONG)]
    rc = RECT(0, 0, mw, mh)
    u32.FillRect(hdc, ctypes.byref(rc), g32.CreateSolidBrush(0x00FFFFFF))
    srcdc = g32.CreateCompatibleDC(None)
    oldsrc = g32.SelectObject(srcdc, ii.hbmMask)
    g32.BitBlt(hdc, 0, 0, mw, mh, srcdc, 0, 0, 0x00CC0020)  # SRCCOPY
    g32.SelectObject(srcdc, oldsrc); g32.DeleteDC(srcdc)
    rgba = dib_to_rgba(hdc, hbm, mw, mh)
    rgba = upscale(rgba, mw, mh, 8)
    png_write_rgba(os.path.join(OUT, name), mw * 8, mh * 8, rgba)
    g32.SelectObject(hdc, old); g32.DeleteObject(hbm); g32.DeleteDC(hdc)
    g32.DeleteObject(ii.hbmMask)
    if ii.hbmColor: g32.DeleteObject(ii.hbmColor)
    print("wrote", name)

# ---------------- 1) tray-icon crate's exact CreateIcon path ------------------
w, h, rgba = png_decode_rgba(TRAY_PNG)
print(f"decoded {os.path.basename(TRAY_PNG)}: {w}x{h}")

px = bytearray(rgba)
mask = bytearray()
for i in range(0, len(px), 4):
    a = px[i+3]
    mask.append((a - 255) & 0xFF)          # crate: pixel.a.wrapping_sub(u8::MAX)
    px[i], px[i+2] = px[i+2], px[i]        # crate: convert_to_bgra

broken = u32.CreateIcon(None, w, h, 1, 32, bytes(mask), bytes(px))
print("broken HICON:", hex(broken) if broken else None)
if broken:
    dump_mask(broken, "broken_mask.png")
    render_icon(broken, 20, 20, 0x00FFFFFF, "broken_on_white_20.png")
    render_icon(broken, 40, 40, 0x00FFFFFF, "broken_on_white_40.png")
    u32.DestroyIcon(broken)

# ---------------- 2) correct CreateIconIndirect for contrast ------------------
def build_correct(w, h, rgba):
    px = bytearray(rgba)
    for i in range(0, len(px), 4):
        px[i], px[i+2] = px[i+2], px[i]
    rowbytes = ((w + 15) // 16) * 2
    m = bytearray(rowbytes * h)
    for y in range(h):
        for x in range(w):
            if rgba[(y*w + x)*4 + 3] < 128:
                m[y*rowbytes + x // 8] |= 0x80 >> (x % 8)
    hdc, hbm_color, bits, old = make_dib(w, h)
    buf = (ctypes.c_ubyte * (w*h*4)).from_address(bits.value)
    for i in range(w*h):  # DIB is BGRA, top-down here
        buf[i*4:i*4+4] = bytes((px[i*4], px[i*4+1], px[i*4+2], px[i*4+3]))
    g32.SelectObject(hdc, old); g32.DeleteDC(hdc)
    hbm_mask = g32.CreateBitmap(w, h, 1, 1, bytes(m))
    ii = ICONINFO(True, 0, 0, hbm_mask, hbm_color)
    hicon = u32.CreateIconIndirect(ctypes.byref(ii))
    g32.DeleteObject(hbm_mask); g32.DeleteObject(hbm_color)
    return hicon

good = build_correct(w, h, rgba)
print("correct HICON:", hex(good) if good else None)
if good:
    render_icon(good, 20, 20, 0x00FFFFFF, "correct_on_white_20.png")
    render_icon(good, 40, 40, 0x00FFFFFF, "correct_on_white_40.png")
    u32.DestroyIcon(good)

# ---------------- 3) exe icon via shell extraction APIs -----------------------
class SHFILEINFOW(ctypes.Structure):
    _fields_ = [("hIcon", W.HICON), ("iIcon", ctypes.c_int),
                ("dwAttributes", W.DWORD),
                ("szDisplayName", ctypes.c_wchar * 260),
                ("szTypeName", ctypes.c_wchar * 80)]

def shgfi(flags, name, size):
    shfi = SHFILEINFOW()
    s32.SHGetFileInfoW(EXE, 0, ctypes.byref(shfi), ctypes.sizeof(shfi), 0x100 | flags)
    if shfi.hIcon:
        render_icon(shfi.hIcon, size, size, 0x00FFFFFF, name)
        u32.DestroyIcon(shfi.hIcon)
    else:
        print(name, "-> no icon")

shgfi(0x1, "exe_shgetfileinfo_small.png", 20)   # SHGFI_SMALLICON
shgfi(0x0, "exe_shgetfileinfo_large.png", 40)   # SHGFI_LARGEICON

hl = W.HICON(); hs = W.HICON()
n = s32.ExtractIconExW(EXE, 0, ctypes.byref(hl), ctypes.byref(hs), 1)
print("ExtractIconEx count:", n)
if hl: render_icon(hl, 40, 40, 0x00FFFFFF, "exe_extracticonex_large.png"); u32.DestroyIcon(hl)
if hs: render_icon(hs, 20, 20, 0x00FFFFFF, "exe_extracticonex_small.png"); u32.DestroyIcon(hs)

for sz in (20, 32):
    arr = (W.HICON * 1)()
    cnt = u32.PrivateExtractIconsW(EXE, 0, sz, sz, arr, None, 1, 0)
    print(f"PrivateExtractIcons {sz}x{sz} count:", cnt)
    if cnt and arr[0]:
        render_icon(arr[0], sz, sz, 0x00FFFFFF, f"exe_privateextract_{sz}.png")
        u32.DestroyIcon(arr[0])

# ---------------- 4) alternative render paths on the broken HICON -------------
# Re-create the broken HICON (previous one was destroyed above).
broken = u32.CreateIcon(None, w, h, 1, 32, bytes(mask), bytes(px))
print("broken HICON #2:", hex(broken) if broken else None)

if broken:
    # (a) legacy two-step: SRCAND the AND mask, then SRCPAINT the color bitmap
    ii = ICONINFO()
    u32.GetIconInfo(broken, ctypes.byref(ii))
    hdc, hbm, bits, old = make_dib(20, 20)
    class RECT(ctypes.Structure):
        _fields_ = [("left", W.LONG), ("top", W.LONG), ("right", W.LONG), ("bottom", W.LONG)]
    rc = RECT(0, 0, 20, 20)
    u32.FillRect(hdc, ctypes.byref(rc), g32.CreateSolidBrush(0x00FFFFFF))
    srcdc = g32.CreateCompatibleDC(None)
    oldsrc = g32.SelectObject(srcdc, ii.hbmMask)
    g32.BitBlt(hdc, 0, 0, 20, 20, srcdc, 0, 0, 0x00220326)  # SRCAND
    g32.SelectObject(srcdc, ii.hbmColor)
    g32.BitBlt(hdc, 0, 0, 20, 20, srcdc, 0, 0, 0x00EE0086)  # SRCPAINT
    g32.SelectObject(srcdc, oldsrc); g32.DeleteDC(srcdc)
    rgba2 = dib_to_rgba(hdc, hbm, 20, 20)
    png_write_rgba(os.path.join(OUT, "broken_legacy_twostep.png"), 160, 160, upscale(rgba2, 20, 20, 8))
    print("wrote broken_legacy_twostep.png")
    g32.SelectObject(hdc, old); g32.DeleteObject(hbm); g32.DeleteDC(hdc)

    # (b) raw BitBlt of hbmColor, no alpha, no mask
    hdc, hbm, bits, old = make_dib(20, 20)
    u32.FillRect(hdc, ctypes.byref(rc), g32.CreateSolidBrush(0x00FFFFFF))
    srcdc = g32.CreateCompatibleDC(None)
    oldsrc = g32.SelectObject(srcdc, ii.hbmColor)
    g32.BitBlt(hdc, 0, 0, 20, 20, srcdc, 0, 0, 0x00CC0020)  # SRCCOPY
    g32.SelectObject(srcdc, oldsrc); g32.DeleteDC(srcdc)
    rgba2 = dib_to_rgba(hdc, hbm, 20, 20)
    png_write_rgba(os.path.join(OUT, "broken_color_bitblt.png"), 160, 160, upscale(rgba2, 20, 20, 8))
    print("wrote broken_color_bitblt.png")
    g32.SelectObject(hdc, old); g32.DeleteObject(hbm); g32.DeleteDC(hdc)
    g32.DeleteObject(ii.hbmMask); g32.DeleteObject(ii.hbmColor)

    # (c) ImageList round-trip (used by legacy shell lists)
    c32 = ctypes.windll.comctl32
    c32.ImageList_Create.restype = W.HANDLE
    c32.ImageList_Create.argtypes = [ctypes.c_int] * 2 + [W.UINT] + [ctypes.c_int] * 2
    c32.ImageList_ReplaceIcon.argtypes = [W.HANDLE, ctypes.c_int, W.HICON]
    c32.ImageList_Draw.argtypes = [W.HANDLE, ctypes.c_int, W.HDC,
                                   ctypes.c_int, ctypes.c_int, W.UINT]
    himl = c32.ImageList_Create(20, 20, 0x21, 1, 1)  # ILC_COLOR32|ILC_MASK
    idx = c32.ImageList_ReplaceIcon(himl, -1, broken)
    print("ImageList_ReplaceIcon ->", idx)
    hdc, hbm, bits, old = make_dib(20, 20)
    u32.FillRect(hdc, ctypes.byref(rc), g32.CreateSolidBrush(0x00FFFFFF))
    c32.ImageList_Draw(himl, 0, hdc, 0, 0, 0)
    rgba2 = dib_to_rgba(hdc, hbm, 20, 20)
    png_write_rgba(os.path.join(OUT, "broken_imagelist.png"), 160, 160, upscale(rgba2, 20, 20, 8))
    print("wrote broken_imagelist.png")
    g32.SelectObject(hdc, old); g32.DeleteObject(hbm); g32.DeleteDC(hdc)
    c32.ImageList_Destroy(himl)

    u32.DestroyIcon(broken)

print("done ->", OUT)
