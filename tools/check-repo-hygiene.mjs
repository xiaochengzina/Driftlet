#!/usr/bin/env node
/**
 * check-repo-hygiene.mjs —— 仓库卫生检查（CI 与本地共用，零依赖 Node 脚本）。
 *
 * 把 2026-09 全量文档审查发现的「靠人记」漂移项固化成自动闸门：
 *   1. 版本号四处一致（package.json / package-lock.json ×2 / Cargo.toml / tauri.conf.json）
 *   2. i18n 中英键对等（src/js/i18n.js 两个字典的键集合逐一对称）
 *   3. 三对双版文档结构对拍（章节数与代码块数两版相等——不钉死数字，等式即不变量）
 *   4. CHANGELOG 顶部形态（[Unreleased] 或与 package.json 同版本的 [X.Y.Z] - 日期）
 *   5. AGENTS.md 约定 #1 点名的六个双版文档文件全部存在
 *
 * 用法：node tools/check-repo-hygiene.mjs（任一检查失败即 exit 1）。
 * 注意：package-lock.json 含空字符串键，pwsh 的 ConvertFrom-Json 会炸——所以用 Node。
 */
import { readFile, access } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const failures = [];
const ok = (msg) => console.log("  ✓ " + msg);
const bad = (msg) => { failures.push(msg); console.log("  ✗ " + msg); };
const FENCE = /^\x60\x60\x60/gm; // 三个反引号开头的行（代码块围栏）

// ─── 1. 版本号四处一致 ───
console.log("[1/5] 版本号四处一致");
{
  const pkg = JSON.parse(await readFile(join(root, "package.json"), "utf8"));
  const lock = JSON.parse(await readFile(join(root, "package-lock.json"), "utf8"));
  const cargo = await readFile(join(root, "src-tauri/Cargo.toml"), "utf8");
  const conf = JSON.parse(await readFile(join(root, "src-tauri/tauri.conf.json"), "utf8"));
  const cargoVer = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  const spots = {
    "package.json": pkg.version,
    "package-lock.json (name)": lock.version,
    "package-lock.json (packages root)": lock.packages?.[""]?.version,
    "src-tauri/Cargo.toml": cargoVer,
    "src-tauri/tauri.conf.json": conf.version,
  };
  const vals = new Set(Object.values(spots));
  if (vals.size === 1 && pkg.version) ok("五处全部 = " + pkg.version);
  else bad("版本号不一致：" + Object.entries(spots).map(([k, v]) => k + "=" + v).join(", "));
}

// ─── 2. i18n 中英键对等 ───
console.log("[2/5] i18n 中英键对等");
{
  const src = await readFile(join(root, "src/js/i18n.js"), "utf8");
  const starts = [...src.matchAll(/^  ('zh-CN'|en): \{/gm)];
  const blocks = {};
  for (let i = 0; i < starts.length; i++) {
    const name = starts[i][1].replaceAll("'", "");
    const s = starts[i].index + starts[i][0].length;
    const e = i + 1 < starts.length ? starts[i + 1].index : src.indexOf("\n};", s);
    const body = src.slice(s, e);
    blocks[name] = new Set([...body.matchAll(/^    '([a-zA-Z0-9._-]+)':/gm)].map((m) => m[1]));
  }
  const zh = blocks["zh-CN"], en = blocks["en"];
  if (!zh || !en) bad("语言块解析失败（期望 'zh-CN' 与 en 两块）");
  else {
    const onlyZh = [...zh].filter((k) => !en.has(k));
    const onlyEn = [...en].filter((k) => !zh.has(k));
    if (!onlyZh.length && !onlyEn.length) ok("zh-CN " + zh.size + " 键 = en " + en.size + " 键，零单边键");
    else bad("单边键：仅 zh " + JSON.stringify(onlyZh) + " / 仅 en " + JSON.stringify(onlyEn));
  }
}

// ─── 3. 三对双版文档结构对拍 ───
console.log("[3/5] 双版文档结构对拍（## 章节数 / 代码块数）");
{
  const pairs = [
    ["docs/皮肤开发指南.md", "docs/skin-development-guide.md"],
    ["docs/关键机制.md", "docs/critical-mechanisms.md"],
    ["docs/架构与机制总览.md", "docs/architecture-and-mechanisms.md"],
  ];
  for (const [zh, en] of pairs) {
    let a, b;
    try {
      a = await readFile(join(root, zh), "utf8");
      b = await readFile(join(root, en), "utf8");
    } catch {
      bad("文件缺失：" + zh + " 或 " + en);
      continue;
    }
    const count = (s, re) => (s.match(re) || []).length;
    const metrics = [["## 章节", /^## /gm], ["代码块围栏", FENCE]];
    const diffs = metrics.map(([l, re]) => [l, count(a, re), count(b, re)]).filter(([, x, y]) => x !== y);
    if (!diffs.length) ok(zh + " ↔ " + en + "（章节 " + count(a, /^## /gm) + "、围栏 " + count(a, FENCE) + " 两版相等）");
    else bad(zh + " ↔ " + en + " 失同步：" + diffs.map(([l, x, y]) => l + " " + x + " vs " + y).join("，"));
  }
}

// ─── 4. CHANGELOG 顶部形态（仅开发仓库——CHANGELOG.md 不同步公开仓库，缺失即跳过）───
console.log("[4/5] CHANGELOG 顶部形态（公开仓库无此文件则跳过）");
{
  const pkg = JSON.parse(await readFile(join(root, "package.json"), "utf8"));
  let cl;
  try {
    cl = await readFile(join(root, "CHANGELOG.md"), "utf8");
  } catch {
    ok("CHANGELOG.md 不存在（公开仓库形态）——跳过本项");
  }
  if (cl) {
  const top = cl.match(/^## \[(.+)\]\s*(?:-\s*(\d{4}-\d{2}-\d{2}))?/m);
  if (!top) bad("找不到版本节标题（## […]）");
  else if (top[1] === "Unreleased") ok("顶部为 [Unreleased]（开发中形态）");
  else if (top[1] === pkg.version && top[2]) ok("顶部 [" + top[1] + "] - " + top[2] + "，与 package.json 一致");
  else bad("顶部节 [" + top[1] + "] 与 package.json 版本 " + pkg.version + " 不符（或缺日期）");
  }
}

// ─── 5. AGENTS.md 约定 #1 点名文件存在 ───
console.log("[5/5] AGENTS.md 约定 #1 点名文件存在");
{
  const pairs = [
    "docs/皮肤开发指南.md", "docs/skin-development-guide.md",
    "docs/关键机制.md", "docs/critical-mechanisms.md",
    "docs/架构与机制总览.md", "docs/architecture-and-mechanisms.md",
  ];
  let missing = 0;
  for (const rel of pairs) {
    try { await access(join(root, rel)); }
    catch { bad("约定 #1 点名文件不存在：" + rel); missing++; }
  }
  if (!missing) ok("三对双版文档六个文件全部存在");
}

console.log("");
if (failures.length) {
  console.error("仓库卫生检查未通过（" + failures.length + " 项）：");
  failures.forEach((f) => console.error("  - " + f));
  process.exit(1);
}
console.log("仓库卫生检查全部通过。");
