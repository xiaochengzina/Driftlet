#!/usr/bin/env node
/**
 * build-offline.mjs —— 离线版安装包一键构建（本地与 CI 同一入口，npm run
 * build:offline 的实现）。
 *
 * 为什么需要编排而不是裸跑 tauri build：bundler 永远先写标准名
 *（Driftlet_<版本>_x64-setup.exe）再轮到改名——「先产出再改名」会让先前
 * 编译好的标准包在改名之前就被覆盖（实机踩过）。本脚本的顺序：
 *   ① 标准名产物已存在 → 先挪到 <名>.preserve-tmp 保护起来；
 *   ② tauri build（offlineInstaller 覆盖——打包时联网拉一次微软离线运行时内嵌）；
 *   ③ 改名离线版为 <-offline> 后缀（rename-offline-installer.mjs 同一来源）；
 *   ④ 恢复被保护的标准包（构建失败也恢复——try/finally）。
 * 成功时 stdout 末行打印离线版产物路径（CI 捕获用）。
 */
import { existsSync, readFileSync, renameSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { renameOfflineInstaller } from "./rename-offline-installer.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const version = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8")).version;
const dir = join(root, "src-tauri/target/release/bundle/nsis");
const standard = join(dir, `Driftlet_${version}_x64-setup.exe`);
const preserved = join(dir, `Driftlet_${version}_x64-setup.exe.preserve-tmp`);

// ① 保护既有标准包（没有则无事）
const hasPreserved = existsSync(standard);
if (hasPreserved) {
  renameSync(standard, preserved);
  console.log(`[build:offline] 已保护既有标准产物：${standard}`);
}

try {
  // ② 构建（直调 tauri CLI 的 JS 入口——不经 npm，少一层 shell 转义面；
  // stdio 继承——输出直通控制台）
  const tauriCli = join(root, "node_modules/@tauri-apps/cli/tauri.js");
  const r = spawnSync(process.execPath, [tauriCli, "build", "--config", "src-tauri/tauri.offline.conf.json"], { stdio: "inherit" });
  if (r.error) throw r.error;
  if (r.status !== 0) throw new Error(`tauri build 失败（exit ${r.status}）`);
  // ③ 改名（命名规则单一来源）
  const out = renameOfflineInstaller(root);
  if (!out) throw new Error("改名失败：找不到标准名产物");
  console.log(`[build:offline] 离线版产物：${out}`);
  console.log(out); // 末行 = 路径（CI 捕获）
} finally {
  // ④ 恢复标准包（成功失败都恢复）
  if (hasPreserved && existsSync(preserved)) {
    renameSync(preserved, standard);
    console.log(`[build:offline] 已恢复标准产物：${standard}`);
  }
}
