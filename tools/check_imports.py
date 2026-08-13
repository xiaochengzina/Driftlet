# check_imports.py — 静态核对 PE 导入表：找出「DLL 存在但缺少导出函数」的条目
# （STATUS_ENTRYPOINT_NOT_FOUND / 0xc0000139 的直接病因）
# 用法: python check_imports.py <exe路径>   （依赖: pip install pefile）
import os
import sys

import pefile

SYSTEM32 = os.path.join(os.environ.get("SystemRoot", r"C:\Windows"), "System32")
SEARCH_DIRS = []  # exe 目录优先，再 System32，再 PATH
_export_cache = {}


def resolve_dll(name, exe_dir):
    for d in [exe_dir, SYSTEM32] + os.environ.get("PATH", "").split(os.pathsep):
        if not d:
            continue
        p = os.path.join(d, name)
        if os.path.isfile(p):
            return p
    return None


def exports_of(dll_path):
    """返回 {函数名: 转发目标或 None}；无法解析返回 None。"""
    key = dll_path.lower()
    if key in _export_cache:
        return _export_cache[key]
    result = None
    try:
        pe = pefile.PE(dll_path, fast_load=True)
        pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_EXPORT"]])
        if hasattr(pe, "DIRECTORY_ENTRY_EXPORT"):
            result = {}
            for exp in pe.DIRECTORY_ENTRY_EXPORT.symbols:
                if not exp.name:
                    continue
                name = exp.name.decode("ascii", "replace")
                fwd = exp.forwarder.decode("ascii", "replace") if exp.forwarder else None
                result[name] = fwd
        else:
            result = {}
    except Exception:
        result = None
    _export_cache[key] = result
    return result


def check_exe(path, depth=0):
    exe_dir = os.path.dirname(os.path.abspath(path))
    pe = pefile.PE(path, fast_load=True)
    pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"]])
    missing = []
    if not hasattr(pe, "DIRECTORY_ENTRY_IMPORT"):
        return missing
    for entry in pe.DIRECTORY_ENTRY_IMPORT:
        dll = entry.dll.decode("ascii", "replace")
        low = dll.lower()
        # API set 虚拟 DLL 由 ApiSetSchema 重定向，静态无法核对，跳过
        if low.startswith("api-ms-") or low.startswith("ext-ms-"):
            continue
        dll_path = resolve_dll(dll, exe_dir)
        if not dll_path:
            # 找不到 DLL 是另一类错误（0xc0000135），仅提示
            print(f"  [warn] DLL 未找到: {dll}")
            continue
        exp = exports_of(dll_path)
        if exp is None:
            print(f"  [warn] 无法解析导出表: {dll_path}")
            continue
        for imp in entry.imports:
            if imp.name is None:  # 按序号导入，跳过
                continue
            fname = imp.name.decode("ascii", "replace")
            if fname in exp:
                fwd = exp[fname]
                if fwd and depth < 4:
                    # 转发导出：KERNELBASE.Foo → 递归核对目标 DLL
                    target_dll, _, target_fn = fwd.partition(".")
                    tpath = resolve_dll(target_dll + ".dll", exe_dir)
                    if tpath:
                        texp = exports_of(tpath)
                        if texp is not None and target_fn not in texp:
                            missing.append((dll, fname, f"转发 {fwd} 缺失"))
                    else:
                        print(f"  [warn] 转发目标 DLL 未找到: {target_dll}.dll (来自 {dll}!{fname})")
            else:
                missing.append((dll, fname, "导出缺失"))
    return missing


def main():
    target = sys.argv[1]
    print(f"检查 {target}")
    missing = check_exe(target)
    if missing:
        print("\n!!! 缺失的导入（病因）：")
        for dll, fn, why in missing:
            print(f"  {dll} -> {fn}  [{why}]")
        sys.exit(1)
    print("\n导入表全部可解析（问题不在静态导入）")


if __name__ == "__main__":
    main()
