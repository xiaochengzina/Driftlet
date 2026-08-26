# check-pack-skin-mirror.py —— pack-skin 镜像一致性对拍（AGENTS.md 硬约定 #2 的 CI 护栏）
#
# types.rs（SkinManifest / SkinSettingKind / SkinSettingDef / SkinSettingOption /
# WindowDefaults + 默认值）、loader.rs（校验函数 + window 默认值钳制）、
# package.rs 安全上限、update.rs parse_version 在 tools/pack-skin/src/main.rs
# 有手工镜像。本脚本逐项对拍，漂移即退出码 1（CI 与本地均可跑）。
#
# 用法: python tools/check-pack-skin-mirror.py

import re
import sys
from pathlib import Path

# CI 的 Windows runner 控制台是 cp1252——中文输出会 UnicodeEncodeError
# （GitHub CI 实证）。输出流统一按 UTF-8 重配，不可编码的字符降级为
# 替换符——输出绝不允许崩掉检查进程。
sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src-tauri" / "src"
TYPES = (SRC / "skin" / "types.rs").read_text(encoding="utf-8")
LOADER = (SRC / "skin" / "loader.rs").read_text(encoding="utf-8")
PACKAGE = (SRC / "skin" / "package.rs").read_text(encoding="utf-8")
COMMANDS = (SRC / "commands.rs").read_text(encoding="utf-8")
UPDATE = (SRC / "update.rs").read_text(encoding="utf-8")
MIRROR = (ROOT / "tools" / "pack-skin" / "src" / "main.rs").read_text(encoding="utf-8")

failures = []


def check(label, ok, detail=""):
    print(f"{'PASS' if ok else 'FAIL'}  {label}" + (f" — {detail}" if detail and not ok else ""))
    if not ok:
        failures.append(label)


def block(text, start_pat):
    """从 start_pat 命中处起做花括号配对，返回整个块文本。
    配对着色前先遮盖注释与字符串/字符字面量（复审 D-D/C-F3：
    裸数花括号会被 `let c = '}';` 这类字面量截短块边界）。"""
    m = re.search(start_pat, text)
    if not m:
        return None
    i = text.index("{", m.start())
    # 遮盖层：与原文等长，注释/字面量内容替换为空格（保位）
    masked = list(text)
    j = 0
    n = len(text)
    while j < n:
        if text.startswith("//", j):
            k = text.find("\n", j)
            k = n if k == -1 else k
            for p in range(j, k):
                masked[p] = " "
            j = k
        elif text.startswith("/*", j):
            k = text.find("*/", j + 2)
            k = n if k == -1 else k + 2
            for p in range(j, k):
                masked[p] = " "
            j = k
        elif text[j] in "\"'":
            q = text[j]
            k = j + 1
            while k < n:
                if text[k] == "\\":
                    k += 2
                    continue
                if text[k] == q:
                    k += 1
                    break
                k += 1
            for p in range(j, min(k, n)):
                masked[p] = " "
            j = k
        else:
            j += 1
    masked = "".join(masked)
    depth = 0
    for j in range(i, n):
        if masked[j] == "{":
            depth += 1
        elif masked[j] == "}":
            depth -= 1
            if depth == 0:
                return text[m.start():j + 1]
    return None


def struct_fields(text, name):
    """结构体字段 → {字段名: serde default 形态}（注释/文档行忽略）。"""
    b = block(text, rf"struct\s+{name}\s*\{{")
    if b is None:
        return None
    out = {}
    pending_attr = ""
    for line in b.splitlines()[1:]:
        line = line.strip()
        if line.startswith("#["):
            # 多个连续属性行叠加（如 #[serde(default)] + #[allow(dead_code)]），
            # 只取其中的 serde 部分参与对拍
            pending_attr += " " + line
            continue
        if line.startswith(("//", "///")) or not line:
            continue  # 文档/空行不断开 attr→字段 的归属
        # 字段名取到冒号为止，类型文本不参与匹配（复审 D-D/C-F2：旧正则
        # `[^,]+,?$` 吃不下 HashMap<String, String> 这类含逗号类型，会把
        # 字段静默丢弃、并把上一字段的 serde 属性错挂到下一字段）
        m = re.match(r"(?:pub\s+)?(\w+)\s*:", line)
        if m:
            attr = ""
            if "default" in pending_attr:
                dm = re.search(r'default\s*=\s*"(\w+)"', pending_attr)
                attr = dm.group(1) if dm else "default"
            rm = re.search(r'rename\s*=\s*"(\w+)"', pending_attr)
            if rm:
                attr += f" rename={rm.group(1)}"
            out[m.group(1)] = attr
        # 字段行不命中（兜底）与字段已消费，都要清零待挂属性
        pending_attr = ""
    return out


def enum_variants(text, name):
    b = block(text, rf"enum\s+{name}\s*\{{")
    if b is None:
        return None
    out = []
    for line in b.splitlines()[1:]:
        # 先剥行尾注释再匹配（复审 D-D：带注释的变体曾被整行丢弃）
        line = line.split("//")[0].strip()
        m = re.match(r"^(\w+),?$", line)
        if m and not line.startswith("#"):
            out.append(m.group(1))
    return out


def fn_tokens(text, name):
    """函数体 token 流（剥注释与全部空白）——校验函数应逐字一致。"""
    b = block(text, rf"fn\s+{name}\s*\(")
    if b is None:
        return None
    b = re.sub(r"//[^\n]*", "", b)
    b = re.sub(r"/\*.*?\*/", "", b, flags=re.S)
    return re.sub(r"\s+", "", b)


def const_value(text, name):
    m = re.search(rf"const\s+{name}\s*:\s*\w+\s*=\s*([^;]+);", text)
    if not m:
        return None
    expr = m.group(1).split("//")[0].strip().replace("_", "")
    if not re.fullmatch(r"[0-9+* ]+", expr):
        return ("unparsed", expr)
    return eval(expr)  # 仅数字与 +/*（上面已白名单化）


def default_fn_value(text, fname):
    """default_width() 等单行/多行函数的返回值文本。"""
    m = re.search(rf"fn\s+{fname}\s*\(\)\s*->\s*\w+\s*\{{\s*([^{{}}]+?)\s*\}}", text, re.S)
    return m.group(1).strip() if m else None


# ─── 1. 结构体字段对拍 ───
for name in ["SkinManifest", "SkinSettingOption", "SkinSettingDef", "WindowDefaults"]:
    a, b = struct_fields(TYPES, name), struct_fields(MIRROR, name)
    if a is None or b is None:
        check(f"{name} 字段", False, "一侧找不到结构体定义")
        continue
    only_a = {k: v for k, v in a.items() if k not in b}
    only_b = {k: v for k, v in b.items() if k not in a}
    attr_diff = {k: (a[k], b[k]) for k in a if k in b and a[k] != b[k]}
    check(f"{name} 字段与 serde 属性", not (only_a or only_b or attr_diff),
          f"仅安装端: {only_a} / 仅 pack-skin: {only_b} / 属性不一致: {attr_diff}")

# ─── 2. SkinSettingKind 变体（保序） ───
a, b = enum_variants(TYPES, "SkinSettingKind"), enum_variants(MIRROR, "SkinSettingKind")
check("SkinSettingKind 22 变体保序一致", a == b and a is not None,
      f"安装端 {a} vs pack-skin {b}" if a != b else "")
# rename_all 判定限定在枚举声明之前的紧邻属性区（复审 D-D/C-F4：全文件
# 子串判定会被别处的同名属性骗过）
check("SkinSettingKind rename_all = lowercase",
      ('#[serde(rename_all = "lowercase")]' in TYPES[:TYPES.find("enum SkinSettingKind")][-200:])
      == ('#[serde(rename_all = "lowercase")]' in MIRROR[:MIRROR.find("enum SkinSettingKind")][-200:]))

# ─── 3. WindowDefaults 默认值函数 ───
for fname in ["default_entry", "default_width", "default_height", "default_opacity", "default_zoom", "default_true"]:
    a, b = default_fn_value(TYPES, fname), default_fn_value(MIRROR, fname)
    check(f"{fname} 默认值一致", a is not None and a == b, f"安装端 {a!r} vs pack-skin {b!r}")

# ─── 4. 安全上限 ───
for cname in ["MAX_PACKAGE_BYTES", "MAX_TOTAL_BYTES", "MAX_FILES"]:
    a, b = const_value(PACKAGE, cname), const_value(MIRROR, cname)
    check(f"{cname} 一致", a == b and a is not None, f"安装端 {a} vs pack-skin {b}")
a, b = const_value(LOADER, "MAX_MANIFEST_BYTES"), const_value(MIRROR, "MAX_MANIFEST_BYTES")
if a is None:  # loader 里的上限可能换个写法/位置——允许从 loader 全文找
    m = re.search(r"1024\s*\*\s*1024", LOADER)
    a = 1024 * 1024 if m else None
check("MAX_MANIFEST_BYTES 一致", a == b and b is not None, f"安装端 {a} vs pack-skin {b}")

# ─── 4b. 打包 skip 清单（package.rs create_package ↔ pack-skin collect）───
def const_str_array(text, name):
    m = re.search(rf"const {name}:\s*\[&str;\s*\d+\]\s*=\s*\[(.*?)\];", text, re.S)
    if not m:
        return None
    return sorted(re.findall(r'"([^"]+)"', m.group(1)))


for cname in ["SKIP_FILES", "SKIP_DIRS"]:
    a, b = const_str_array(PACKAGE, cname), const_str_array(MIRROR, cname)
    check(f"{cname} 清单一致（create_package ↔ pack-skin collect）",
          a is not None and a == b, f"安装端 {a} vs pack-skin {b}")

# ─── 5. 校验函数逐字对拍（loader.rs ↔ main.rs） ───
for fname in ["validate_skin_id", "is_reserved_device_name", "is_valid_entry_name"]:
    a, b = fn_tokens(LOADER, fname), fn_tokens(MIRROR, fname)
    # 安装端有 i18n 错误文案（trf/lang）而 pack-skin 返回 bool——只强行对拍
    # 纯函数 is_reserved_device_name / is_valid_entry_name；validate_skin_id
    # 两侧签名不同（Result vs bool），改为对拍保留名单与字符集规则要点
    # 两侧签名不同（Result vs bool），对拍规则要点：字符集、保留名、长度上限
    if fname == "validate_skin_id":
        ok = (a is not None and b is not None
              and "is_reserved_device_name" in a and "is_reserved_device_name" in b
              and "is_ascii_lowercase" in a and "is_ascii_lowercase" in b
              and "len()<=64" in a.replace(" ", "") and "len()<=64" in b.replace(" ", ""))
        check("validate_skin_id 规则要点（字符集 + 保留名 + 长度上限 64）", ok)
    else:
        check(f"{fname} 逐字一致", a == b and a is not None, "函数体 token 流不一致")

# ─── 6. window 默认值钳制镜像（loader.rs ↔ main.rs 字面量） ───
clamp_pairs = [
    ("宽高 [1,10000]", r"clamp\(1,\s*10000\)"),
    ("opacity [0.1,1.0]", r"clamp\(0\.1,\s*1\.0\)"),
    ("refresh_seconds ≤24h", r"min\(86400\)"),
]
for label, pat in clamp_pairs:
    check(f"window 钳制镜像：{label}",
          re.search(pat, LOADER) is not None and re.search(pat, MIRROR) is not None)
zoom_lo = re.search(r"MIN_ZOOM[^=\n]*=\s*([\d.]+)", COMMANDS)
zoom_hi = re.search(r"MAX_ZOOM[^=\n]*=\s*([\d.]+)", COMMANDS)
zoom_ok = (zoom_lo and zoom_hi
           and re.search(rf"clamp\({re.escape(zoom_lo.group(1))},\s*{re.escape(zoom_hi.group(1))}\)", MIRROR))
check("zoom 钳制镜像（commands.rs 常量 → pack-skin）", bool(zoom_ok),
      f"安装端 {zoom_lo and zoom_lo.group(1)}..{zoom_hi and zoom_hi.group(1)}")

# ─── 7. parse_version 镜像（update.rs ↔ main.rs） ───
a, b = fn_tokens(UPDATE, "parse_version"), fn_tokens(MIRROR, "parse_version")
check("parse_version 逐字一致", a == b and a is not None, "min_host_version 解析口径两侧不一致")

print()
if failures:
    print(f"镜像漂移 {len(failures)} 项：{'；'.join(failures)}")
    print("修复方式：同步 tools/pack-skin/src/main.rs 并重建 tools/pack-skin.exe（AGENTS.md 约定 #2）")
    sys.exit(1)
print("pack-skin 镜像全部一致")
