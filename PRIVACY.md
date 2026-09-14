# 隐私政策 / Privacy Policy

[中文版](#中文版) | [English](#english)

---

## 中文版

**生效日期：2026 年 7 月 30 日**

Driftlet（以下简称「本应用」）是一款开源的 Windows 桌面皮肤管理器（GPL v3，源码见 [GitHub 仓库](https://github.com/xiaochengzina/Driftlet)）。本政策说明本应用自身的信息处理行为。

### 本应用不收集什么

- 无账号体系、无遥测、无使用统计、无广告追踪；
- 不收集、存储或向任何服务器传输任何可识别个人身份的信息；
- 全部用户数据（配置、皮肤、皮肤设置值）仅保存在本机——便携布局下为 `<安装目录>\config\` 与 `<安装目录>\skins\`，安装目录不可写时回退到 `%APPDATA%\com.driftlet.app\`。安装版卸载时上述应用数据被删除。

### 本应用自身的网络行为（仅有以下几类）

1. **启动时更新检测**（默认开启，可在「设置」中完全关闭）：向 `api.github.com` 查询公开仓库 `xiaochengzina/Driftlet` 的最新 release。请求仅携带 `User-Agent`（形如 `Driftlet/1.2.3`，含应用名与当前版本号），不携带任何用户数据。
2. **安装包下载**（仅在更新检测发现新版本、且经你在更新弹窗中确认后）：从 GitHub releases 或源码中列明的公共镜像前缀下载安装包，全程校验官方 SHA-256。
3. **打开链接**：仅在你点击「前往下载」等按钮时，用系统默认浏览器打开固定地址（GitHub releases 页等）。
4. **安装器**：当系统缺少 WebView2 运行时时，安装程序会从微软官方链接（`go.microsoft.com`）下载其引导程序。

除上述四类外，本应用自身不发起任何网络请求。

### 第三方皮肤

皮肤是你**自行选择安装**的本机代码（`.dskin` 包）。声明了 `network`（网络请求）权限的皮肤可以自行联网——包括访问第三方服务，并可能携带你在该皮肤设置中主动填写的凭据（如 API Key）。安装引导页会逐条展示每个皮肤的权限声明并按风险分级标注，确认后才安装。第三方皮肤作者的数据处理行为不在本政策覆盖范围内，请按皮肤的可信度自行判断。

### 本政策的变更

本政策的修订随公开仓库提交历史公开可查。联系方式：在 [GitHub Issues](https://github.com/xiaochengzina/Driftlet/issues) 提交。

---

## English

**Effective date: July 30, 2026.**

Driftlet ("the app") is an open-source Windows desktop skin manager (GPL v3; source code in the [GitHub repository](https://github.com/xiaochengzina/Driftlet)). This policy describes the information handling of the app itself.

### What the app does not collect

- No accounts, no telemetry, no usage analytics, no ad tracking;
- It does not collect, store, or transmit any personally identifiable information to any server;
- All user data (configuration, skins, skin setting values) stays on your machine — `<install dir>\config\` and `<install dir>\skins\` in the portable layout, falling back to `%APPDATA%\com.driftlet.app\` when the install directory is not writable. The installed build removes this app data on uninstall.

### The app's own network activity (only these four kinds)

1. **Startup update check** (on by default; can be turned off entirely in Settings): queries `api.github.com` for the latest release of the public repository `xiaochengzina/Driftlet`. The request carries only a `User-Agent` header (e.g. `Driftlet/1.2.3`, containing the app name and current version) and no user data.
2. **Installer download** (only after the update check finds a new version and you confirm in the update dialog): downloads the installer from GitHub releases or from the public mirror prefixes listed in the source code, verifying the official SHA-256 throughout.
3. **Opening links**: only when you click buttons such as "Go to download page", the app opens fixed addresses (e.g. the GitHub releases page) in your default browser.
4. **Installer**: if the WebView2 Runtime is missing, the installer downloads its bootstrapper from an official Microsoft link (`go.microsoft.com`).

Beyond these four, the app itself makes no network requests.

### Third-party skins

Skins are local code packages (`.dskin`) that **you choose to install**. A skin declaring the `network` permission may access the network on its own — including third-party services, potentially with credentials you deliberately entered in that skin's settings (such as an API key). The install wizard lists every skin's permission declarations with risk grading before you confirm. Data handling by third-party skin authors is not covered by this policy; judge each skin's trustworthiness yourself.

### Changes to this policy

Revisions of this policy are publicly visible in the public repository's commit history. Contact: file an issue on [GitHub Issues](https://github.com/xiaochengzina/Driftlet/issues).
