<!-- 中文提交信息，conventional 风格（feat:/fix:/refactor:/chore:/docs:） -->

## 改动说明

<!-- 做了什么、为什么；关联 issue 用 Fixes #123 -->

## 自查清单（AGENTS.md 硬性约定）

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` 全绿，`npx vite build` 通过
- [ ] `node tools/check-repo-hygiene.mjs` 通过（版本号四处 / i18n 中英键 / 双版文档结构）
- [ ] 改了对外行为 → 双版文档已同步（皮肤开发指南 / 关键机制 / 架构总览三对按需）；CHANGELOG 由维护者合入后补充（公开仓库不含该文件）
- [ ] 改了 `src-tauri/src/skin/{types,loader,package}.rs` 的校验/上限 → pack-skin 镜像已同步并重建 exe
- [ ] 新增/改了 IPC 命令 → `policy.rs` 的 `COMMAND_POLICIES` 已登记，函数体带对应闸门
- [ ] 新功能 → 已补测试；发布相关改动 → 已通知维护者补实机测试清单条目（仅开发仓库）

## 截图 / 录屏

<!-- UI 改动必附；暗色主题请同时验证 -->
