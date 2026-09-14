#!/usr/bin/env node
/**
 * rename-offline-installer.mjs —— 离线版安装包改名（命名规则的唯一来源）。
 *
 * 离线版与标准版的 NSIS 产物同名（Driftlet_<版本>_x64-setup.exe——tauri 按
 * productName+版本+架构命名，打包配置无改名面），两个产物不能一样：本脚本把
 * 离线版改成 <-offline> 后缀。版本号取 tauri.conf.json（bundler 的命名来源）。
 *
 * 两用：被 tools/build-offline.mjs import（renameOfflineInstaller），或直接
 * 命令行运行（stdout 只打印改名后的相对路径供 CI 捕获，exit 非 0 即失败）。
 */
import { existsSync, readFileSync, renameSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { basename } from "node:path";

export function renameOfflineInstaller(root) {
  const conf = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
  const dir = join(root, "src-tauri/target/release/bundle/nsis");
  const src = join(dir, `Driftlet_${conf.version}_x64-setup.exe`);
  const dst = join(dir, `Driftlet_${conf.version}_x64-setup-offline.exe`);
  if (!existsSync(src)) return null; // 未构建/已改名
  renameSync(src, dst); // 目标已存在则覆盖（同一版本重复构建）
  return dst;
}

// 直接运行（node tools/rename-offline-installer.mjs）
if (process.argv[1] && basename(process.argv[1]) === basename(fileURLToPath(import.meta.url))) {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const out = renameOfflineInstaller(root);
  if (!out) {
    console.error("offline installer not found at expected path（未构建或已改名）");
    process.exit(1);
  }
  console.log(out);
}
