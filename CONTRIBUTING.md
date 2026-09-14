# 贡献指南 / Contributing

[中文版](#中文版) | [English](#english)

---

## 中文版

感谢你的兴趣！Driftlet 是单人维护的开源项目，欢迎 issue 与 PR。

### 我能帮什么

- **报 bug**：用 [Bug 报告模板](https://github.com/xiaochengzina/Driftlet/issues/new?template=bug_report.yml)——复现步骤、版本号、日志（设置 → 高级 → 日志）是三大关键信息。
- **提需求**：用 [功能建议模板](https://github.com/xiaochengzina/Driftlet/issues/new?template=feature_request.yml)。
- **写皮肤**：从 [`docs/皮肤开发指南.md`](docs/皮肤开发指南.md)（[English](docs/skin-development-guide.md)）开始；优秀的第三方皮肤会在 README/官网获得推荐位。
- **提 PR**：先开 issue 讨论方案再动手，能显著提高合入率。

### 开发环境

```bash
npm install
npm run tauri dev                                   # 开发模式（debug 构建）
cargo test --manifest-path src-tauri/Cargo.toml     # 后端全量测试（改动后必跑）
npx vite build                                      # 前端构建
node tools/check-repo-hygiene.mjs                   # 仓库卫生检查（版本号/i18n/双版文档结构）
```

### 硬性约定（摘要，全文见 [AGENTS.md](AGENTS.md)）

1. **文档双版同步**：`docs/皮肤开发指南.md` ↔ `skin-development-guide.md`、`docs/关键机制.md` ↔ `critical-mechanisms.md`、`docs/架构与机制总览.md` ↔ `architecture-and-mechanisms.md` 三对必须成对更新；CHANGELOG.md 仅中文版。
2. **pack-skin 镜像**：改 `src-tauri/src/skin/{types,loader,package}.rs` 的校验/上限时，同步 `tools/pack-skin/src/main.rs` 并重建 exe（CI 有对拍闸）。
3. **版本号四处一致**：package.json / package-lock.json / Cargo.toml / tauri.conf.json。
4. **命令闸门**：任何新 IPC 命令必须在 `src-tauri/src/policy.rs` 的 `COMMAND_POLICIES` 登记档位（完备性测试兜底）；管理器命令首行 `require_manager`。
5. **vendored 补丁**：`src-tauri/vendor/` 内 `NOTE(driftlet)` 标注的补丁升级依赖时必须保留。
6. **行尾**：LF（`*.ps1` 为 CRLF），见 `.editorconfig` / `.gitattributes`。
7. **测试**：`cargo test` 必须全绿；新功能补测试与（开发仓库的）实机测试清单条目。
8. **提交信息**：详细中文 conventional 风格（`feat:` / `fix:` / `refactor:` / `chore:` / `docs:`）。

改窗口/桌面层级相关代码前，请先读 [`docs/关键机制.md`](docs/关键机制.md)（[English](docs/critical-mechanisms.md)）——它是「勿回归」清单；项目全貌见 [`docs/架构与机制总览.md`](docs/架构与机制总览.md)（[English](docs/architecture-and-mechanisms.md)）。

---

## English

Thanks for your interest! Driftlet is a single-maintainer open-source project; issues and PRs are welcome.

### How to help

- **Report bugs**: use the [bug report template](https://github.com/xiaochengzina/Driftlet/issues/new?template=bug_report.yml) — reproduction steps, version number, and logs (Settings → Advanced → Logs) are the three key pieces.
- **Request features**: use the [feature request template](https://github.com/xiaochengzina/Driftlet/issues/new?template=feature_request.yml).
- **Write a skin**: start from the [skin development guide](docs/skin-development-guide.md); outstanding third-party skins get recommended placement.
- **Send PRs**: open an issue to discuss the approach first — it greatly improves merge odds.

### Development setup

```bash
npm install
npm run tauri dev                                   # dev mode (debug build)
cargo test --manifest-path src-tauri/Cargo.toml     # full backend tests (mandatory after changes)
npx vite build                                      # frontend build
node tools/check-repo-hygiene.mjs                   # repo hygiene checks (versions/i18n/doc pairs)
```

### Hard rules (summary; full text in [AGENTS.md](AGENTS.md))

1. **Dual-version docs**: the three pairs (skin guide / critical mechanisms / architecture overview, each zh ↔ en) must be updated together; CHANGELOG.md is Chinese-only.
2. **pack-skin mirror**: when changing validation/limits in `src-tauri/src/skin/{types,loader,package}.rs`, sync `tools/pack-skin/src/main.rs` and rebuild the exe (CI cross-checks it).
3. **Version numbers agree in four places**: package.json / package-lock.json / Cargo.toml / tauri.conf.json.
4. **Command gates**: every new IPC command registers a tier in `COMMAND_POLICIES` in `src-tauri/src/policy.rs` (completeness tests enforce it); manager commands lead with `require_manager`.
5. **Vendored patches**: the `NOTE(driftlet)` patches in `src-tauri/vendor/` must survive dependency upgrades.
6. **Line endings**: LF (`*.ps1` stays CRLF) — see `.editorconfig` / `.gitattributes`.
7. **Tests**: `cargo test` must stay green; new features add tests and (dev-repo) real-machine checklist entries.
8. **Commit messages**: detailed Chinese conventional style (`feat:` / `fix:` / `refactor:` / `chore:` / `docs:`).

Before touching window/desktop-layer code, read [`docs/critical-mechanisms.md`](docs/critical-mechanisms.md) first — it is the do-not-regress list; for the full picture see [`docs/architecture-and-mechanisms.md`](docs/architecture-and-mechanisms.md).
