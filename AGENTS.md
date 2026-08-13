# AGENTS.md

Driftlet —— Windows 桌面挂件平台（Tauri v2 + WebView2 + 原生 JS 管理器，刻意不用前端框架）。

## 常用命令

- `npm run tauri dev`：开发模式跑应用（debug 构建，含皮肤热重载 watcher）
- `npx vite build`：前端构建（多页：index.html + log.html）
- `cargo test --manifest-path src-tauri/Cargo.toml`：后端全量测试（改动后必跑）
- pack-skin 重建：`cargo build --release --manifest-path tools/pack-skin/Cargo.toml`，再把 `target/release/pack-skin.exe` 复制覆盖 `tools/pack-skin.exe`

## 硬性约定

1. **文档双版同步**：`docs/皮肤开发指南.md` ↔ `skin-development-guide.md`、`docs/关键机制.md` ↔ `critical-mechanisms.md` 必须成对更新；CHANGELOG.md 仅中文版。
2. **pack-skin 镜像同步**：`src-tauri/src/skin/types.rs`（SkinManifest / SkinSettingKind / SkinSettingDef / WindowDefaults）、`loader.rs` 的校验函数、`package.rs` 的安全上限，在 `tools/pack-skin/src/main.rs` 有手工镜像——改动必须同步并重建 exe（exe 入库，供创作者免环境使用）。
3. **版本号四处一致**：`package.json` / `package-lock.json` / `src-tauri/Cargo.toml` / `src-tauri/tauri.conf.json`。
4. **命令闸门**：`skin_api/` 新增敏感命令必须过 `require_perm`；管理器命令必须首行 `require_manager`；日志缓冲读取必须过 `require_log_window`。自定义命令对任何窗口开放，身份只认窗口 label。
5. **vendored 补丁**：`src-tauri/vendor/` 内的本地补丁以 `NOTE(driftlet)` 标注（tauri-runtime-wry / tray-icon），升级依赖时必须保留，勿直接覆盖。
6. **行尾**：`.gitattributes` 已定 `* text=auto eol=lf`（`*.ps1` 为 CRLF）。`git status` 出现无内容差异的脏文件时先查行尾。
7. **生成物**：`dist/`、`node_modules/`、`target/` 不入库；`src-tauri/gen/schemas/` 是知情入库（编辑器补全 capabilities 用），插件/capabilities 变更后随构建 regenerate 一并提交。

## 提交与文档

- 提交信息：详细中文 conventional 风格（`feat:` / `fix:` / `refactor:` / `chore:` / `docs:`），涉及关键机制的变更把事故依据写进提交信息或 `docs/关键机制.md`（该文件是「勿回归」清单，不是可选读物）。
- 前端无框架：`src/js/` 原生 JS 模块；转义/弹窗等公共件在 `src/js/dom.js`，不要再复制第三份。
