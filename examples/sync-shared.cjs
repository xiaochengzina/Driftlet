#!/usr/bin/env node
/* 屿 · Isles —— 把 shared/ 基座复制进每个皮肤文件夹（皮肤包必须自包含）。
   用法：node examples/sync-shared.cjs
   规则：examples/ 下所有含 skin.json 的目录都会收到 tokens.css /
   base.css / base.js 的最新副本；shared/ 是唯一事实源，皮肤内副本勿手改。 */
const fs = require("fs");
const path = require("path");

const root = __dirname;
const SHARED = path.join(root, "shared");
const FILES = ["tokens.css", "base.css", "base.js"];

const targets = fs
  .readdirSync(root, { withFileTypes: true })
  .filter((d) => d.isDirectory() && d.name !== "shared")
  .filter((d) => fs.existsSync(path.join(root, d.name, "skin.json")))
  .map((d) => path.join(root, d.name));

if (targets.length === 0) {
  console.log("sync-shared: 没有找到含 skin.json 的皮肤目录");
  process.exit(0);
}

let ok = true;
for (const dir of targets) {
  for (const f of FILES) {
    const from = path.join(SHARED, f);
    const to = path.join(dir, f);
    const data = fs.readFileSync(from);
    if (fs.existsSync(to) && fs.readFileSync(to).equals(data)) continue;
    try {
      fs.writeFileSync(to, data);
      console.log(`  ${path.basename(dir)}/${f}  已同步`);
    } catch (e) {
      console.error(`  ${path.basename(dir)}/${f}  写入失败: ${e.message}`);
      ok = false;
    }
  }
}
console.log(ok ? `sync-shared: ${targets.length} 个皮肤已同步` : "sync-shared: 存在失败项");
process.exit(ok ? 0 : 1);
