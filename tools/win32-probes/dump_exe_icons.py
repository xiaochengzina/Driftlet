# Dump RT_GROUP_ICON / RT_ICON from a PE file for inspection.
import struct, sys, os

path = sys.argv[1]
data = open(path, "rb").read()

e_lfanew = struct.unpack_from("<I", data, 0x3C)[0]
assert data[e_lfanew:e_lfanew+4] == b"PE\0\0"
coff = e_lfanew + 4
num_sections = struct.unpack_from("<H", data, coff + 2)[0]
opt_size = struct.unpack_from("<H", data, coff + 16)[0]
opt = coff + 20
magic = struct.unpack_from("<H", data, opt)[0]
dd_base = opt + (112 if magic == 0x20B else 96)
rsrc_rva, rsrc_size = struct.unpack_from("<II", data, dd_base + 2 * 8)
sec_base = opt + opt_size
sections = []
for i in range(num_sections):
    off = sec_base + 40 * i
    name = data[off:off+8].rstrip(b"\0").decode(errors="replace")
    vsize, vaddr, rawsize, rawptr = struct.unpack_from("<IIII", data, off + 8)
    sections.append((name, vaddr, vsize, rawptr, rawsize))

def rva_to_off(rva):
    for name, vaddr, vsize, rawptr, rawsize in sections:
        if vaddr <= rva < vaddr + max(vsize, rawsize):
            return rawptr + (rva - vaddr)
    raise ValueError(f"RVA {rva:#x} not in any section")

def walk(dir_off, level, out, path_ids=()):
    num_named, num_id = struct.unpack_from("<HH", data, dir_off + 12)
    total = num_named + num_id
    for i in range(total):
        e = dir_off + 16 + 8 * i
        name, val = struct.unpack_from("<II", data, e)
        rid = name & 0xFFFF if not (name & 0x80000000) else f"str@{name & 0x7FFFFFFF}"
        if val & 0x80000000:
            walk(rsrc_base + (val & 0x7FFFFFFF), level + 1, out, path_ids + (rid,))
        else:
            dentry = rsrc_base + val
            drva, dsize = struct.unpack_from("<II", data, dentry)
            out.append((path_ids + (rid,), drva, dsize))

rsrc_base = rva_to_off(rsrc_rva)
entries = []
walk(rsrc_base, 0, entries)

groups = {}
icons = {}
for ids, drva, dsize in entries:
    if ids[0] == 14:
        groups[ids[1]] = (drva, dsize)
    elif ids[0] == 3:
        icons[ids[1]] = (drva, dsize)

print(f"RT_GROUP_ICON ids: {list(groups)}, RT_ICON ids: {sorted(icons)}")
for gid, (drva, dsize) in groups.items():
    goff = rva_to_off(drva)
    gdata = data[goff:goff+dsize]
    reserved, typ, count = struct.unpack_from("<HHH", gdata, 0)
    print(f"GROUP {gid}: count={count}")
    for i in range(count):
        o = 6 + 14 * i  # GRPICONDIR entry = 14 bytes
        w, h, pal, res, planes, bpp, bsize, nid = struct.unpack_from("<BBBBHHIH", gdata, o)
        have = icons.get(nid)
        sig = ""
        if have:
            ioff = rva_to_off(have[0])
            sigb = data[ioff:ioff+8]
            sig = "PNG" if sigb[:4] == b"\x89PNG" else ("BMP" if sigb[:2] == b"BM" or struct.unpack_from('<I', sigb)[0] == 40 else sigb.hex())
            # sanity: declared size vs resource size
            ok = "OK" if have[1] == bsize else f"SIZE-MISMATCH res={have[1]} decl={bsize}"
        else:
            ok = "MISSING RT_ICON!"
        print(f"  entry {i}: {w or 256}x{h or 256} bpp={bpp} bytes={bsize} -> icon id {nid} [{sig}] {ok}")
