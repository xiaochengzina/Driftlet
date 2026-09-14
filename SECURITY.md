# 安全政策 / Security Policy

[中文版](#中文版) | [English](#english)

## 中文版

### 报告漏洞

**请不要在公开 issue 中报告安全漏洞。** 请通过 [GitHub 私密安全通告](https://github.com/xiaochengzina/Driftlet/security/advisories/new) 提交——你会在 72 小时内收到回复。漏洞修复并发布后，报告者将在 release notes 中致谢（除非你希望匿名）。

### 范围

**在范围内**（平台层）：皮肤沙箱逃逸（文件/注册表/Shell 权限绕过声明机制）、权限闸门缺陷（未声明即调用成功）、更新通道完整性（哈希/标记校验绕过）、安装/备份解压的路径穿越、IPC 身份校验绕过。

**不在范围内**（信任模型内行为）：第三方皮肤是**用户自行选择安装的本机代码**——声明了对应权限的皮肤按其权限行事（如 shell 权限皮肤执行命令）属设计行为；皮肤的联网行为见 [PRIVACY.md](PRIVACY.md) 的信任边界说明。

### 支持版本

仅最新发布版接受安全修复。

## English

### Reporting a vulnerability

**Please do not report security vulnerabilities in public issues.** Use [GitHub private security advisories](https://github.com/xiaochengzina/Driftlet/security/advisories/new) — you will hear back within 72 hours. Reporters are credited in the release notes after the fix ships (unless you prefer anonymity).

### Scope

**In scope** (platform layer): skin-sandbox escapes (file/registry/shell access bypassing the declaration mechanism), permission-gate defects (calls succeeding undeclared), update-channel integrity bypasses (hash/marker checks), path traversal in package/backup extraction, IPC identity-check bypasses.

**Out of scope** (trust-model behavior): third-party skins are **local code the user chooses to install** — a skin acting within its declared permissions (e.g. a shell-permissioned skin running commands) is by design; skin network behavior is covered by the trust-boundary section of [PRIVACY.md](PRIVACY.md).

### Supported versions

Only the latest release receives security fixes.
