# Driftlet

> 中文版 | [English](README_EN.md)

一款基于 Tauri 2 + Vite / 原生 JavaScript 构建的 Windows 桌面皮肤管理器。支持将网页以桌面小部件（Widget）形式呈现，提供透明窗口、无边框窗口、窗口置顶以及固定窗口至桌面。

---

## 功能

- 安装、卸载、加载、重新加载皮肤
- 以 `.dskin` 皮肤包（zip 格式）安装/更新皮肤，更新保留用户设置数据
- 双击 `.dskin` 文件直接唤起安装引导页（安装版注册文件关联）
- 皮肤权限模型：敏感能力需在 `skin.json` 声明 `permissions`（注册表 / Shell / 系统控制 / 剪贴板 / 麦克风共 5 种），安装引导页逐条展示并按高危/中危两档分级标注（见「安全模型」）
- 皮肤自定义设置：`skin.json` 声明配置项（20 种控件 + 分组 + 描述），配置面板自动生成
- 调整皮肤的不透明度、位置、大小、缩放比例（「窗口」页可开拖拽缩放：显示边框提示，拖边缘/四角直接调整；缩放比例 50%–200% 整体缩放窗口与内容）
- 窗口放置双态：置顶 / 贴在桌面（默认贴在桌面）、禁止拖动
- 鼠标穿透（皮肤「窗口」页开关，默认关）：点击/滚动穿透到下层窗口或桌面，配合贴桌面即成纯展示挂件
- 为皮肤截取预览图
- 托盘图标管理，主窗口关闭即隐藏到托盘
- 开机自启、暗/亮主题切换
- 右键皮肤窗口打开皮肤菜单（打开配置 / 刷新 / 卸载）
- 全局快捷键一键隐藏/显示已加载的皮肤（默认 Ctrl+Shift+Alt+D，可在设置中修改或禁用），托盘菜单同步勾选项；皮肤窗口按 Alt+F4 只会隐藏，可经快捷键/托盘唤回
- 管理器与皮肤窗口已屏蔽 F5 等浏览器刷新/导航快捷键，页面不可经按键刷新，窗口生命周期完全归管理器
- 布局备份：设置页一键导出/导入全部配置与皮肤（单个 zip 备份文件，适合换机与分享布局）
- 启动更新检测（默认开，可在设置中关闭）：发现 GitHub 新版本时弹窗提示，可一键跳转下载页面或选择不再提示

---

## 环境要求

- Windows 10/11（部分功能依赖 Win32 API）
- Node.js
- Rust / Cargo（Tauri 2 需要）
- WebView2 Runtime（Win11 已自带）

---

## 快速开始

```bash
# 克隆仓库
git clone <仓库地址>
cd Driftlet

# 安装依赖
npm install

# 开发模式
npm run tauri dev

# 构建生产包
npm run tauri build
```

---

## 项目结构

```
├── src/                  # 前端源码
│   ├── js/               # 原生 JS（app.js 入口）
│   └── css/              # 样式
├── src-tauri/src/        # Rust 后端
│   ├── commands.rs       # Tauri IPC 命令（管理器命令统一 require_manager 把关）
│   ├── lib.rs            # 应用启动、状态、自动加载
│   ├── desktop.rs        # Windows "贴在桌面" 实现
│   ├── window/factory.rs # 皮肤窗口创建 / 无边框子类
│   ├── window/snap.rs    # 边缘吸附（WM_MOVING 中就地改写坐标）
│   ├── skin/             # 皮肤扫描、加载、配置、.dskin 包安装
│   └── skin_api/         # 皮肤可调的系统信息与敏感能力命令（require_perm 鉴权）
├── src-tauri/capabilities/ # 窗口权限：default.json（主窗口）/ skin.json（皮肤窗口，空权限）
├── examples/             # 示例皮肤源（参考实现，以独立 .dskin 分发，不随安装包打包）
│   ├── controls-demo/        # 全部设置控件演示（中英双语、界面语言跟随管理器）
│   ├── sys-monitor/          # 系统监视（只读系统信息接口全家桶）
│   ├── media-hub/            # 媒体控制台（音量/媒体/频谱/通知）
│   └── toolbox/              # 本机工具箱（剪贴板/文件/注册表/命令/设置读写）
├── tools/
│   ├── pack-skin.exe     # 皮肤打包工具（免安装，生成 .dskin）
│   ├── pack-skin/        # 打包工具源码（Rust）
│   └── win32-probes/     # Windows 窗口探测脚本（调试用）
├── CHANGELOG.md          # 版本变更记录
└── docs/                 # 开发文档
    ├── 皮肤开发指南.md    # 皮肤创作者接口文档与规范
    ├── 关键机制.md        # 窗口 / 桌面层级实现细节（勿回归）
    ├── 设计系统.md        # 管理器 UI 视觉体系与前端契约
    ├── 已知问题.md        # 已知问题与后续方向
    └── 路线图.md          # 候选新方向与 1.0 发布后实机回归清单
```

---

## 皮肤开发

> 完整接口文档与规范见 [`docs/皮肤开发指南.md`](docs/皮肤开发指南.md)，本节为快速上手。

开发构建（`npm run tauri dev`）下皮肤文件保存后可自动重载（防抖 300ms），无需手动右键刷新；热重载默认关闭，在设置页开启后生效。

皮肤是一个独立文件夹，至少包含：

```
my-skin/
├── skin.json        # 皮肤元信息 / 窗口默认配置
├── index.html       # 入口页面
└── ...              # 图片、css、js 等资源（均在皮肤文件夹内）
```

### skin.json 示例

```json
{
  "id": "my-skin",
  "name": "My Skin",
  "name_en": "My Skin",
  "version": "1.0.0",
  "author": "You",
  "description": "A simple desktop widget",
  "entry": "index.html",
  "window": {
    "width": 300,
    "height": 200,
    "transparent": true,
    "always_on_top": false,
    "on_desktop": true,
    "resizable": false,
    "zoom": 1.0,
    "opacity": 0.95
  }
}
```

### 可拖动区域

在 HTML 中给元素加上 `.drag-region` 类即可拖动皮肤窗口：

```html
<div class="drag-region">
  <!-- 这里的内容可以拖动窗口 -->
</div>
```

### 自适应布局

皮肤窗口尺寸随时可能被用户改变（管理器面板调数值，或 `resizable` 开启后拖拽边框），皮肤布局必须自适应：元素不得超出窗口可视区、不出现窗口级滚动条。规范与两种范式（缩放自适应 / 填充+内部滚动）见 `docs/皮肤开发指南.md` §3.3，示例皮肤已按此改造。

### 调用后端命令

皮肤内可以通过注入的 `window.__DESK_PP__` 调用 Tauri 命令：

```js
if (window.__DESK_PP__?.invoke) {
  const stats = await window.__DESK_PP__.invoke('get_system_stats');
}
```

### 自定义设置

皮肤可以在 `skin.json` 里用 `settings` 数组声明配置项，管理器的配置面板会出现独立的「皮肤设置」页签，按声明自动生成对应控件，值随全局配置持久化：

```json
"settings": [
  { "key": "title",        "type": "text",        "label": "标题",     "default": "Hello" },
  { "key": "notes",        "type": "longtext",    "label": "备注",     "default": "" },
  { "key": "alarm_time",   "type": "time",        "label": "闹钟时间", "default": "07:30" },
  { "key": "start_date",   "type": "date",        "label": "开始日期", "default": "2026-01-01" },
  { "key": "show_seconds", "type": "boolean",     "label": "显示秒针", "default": true },
  { "key": "features",     "type": "multiselect", "label": "启用功能", "default": ["a"],
    "options": [ { "value": "a", "label": "功能 A" }, { "value": "b", "label": "功能 B" } ] },
  { "key": "mode",         "type": "radio",       "label": "模式",     "default": "auto",
    "options": [ { "value": "day", "label": "白天" }, { "value": "night", "label": "夜晚" }, { "value": "auto" } ] },
  { "key": "accent_color", "type": "palette",     "label": "主题色",   "default": "#ff3333",
    "options": [ { "value": "#ff3333" }, { "value": "#4da3ff" } ] },
  { "key": "active_range", "type": "timerange",   "label": "生效时段",
    "default": { "start": "2026-07-20 12:00:00", "end": "2026-08-20 00:00:00" } },
  { "key": "level",        "type": "slider",      "label": "强度",     "default": 60, "min": 0, "max": 100, "step": 1 },
  { "key": "refresh_ms",   "type": "number",      "label": "刷新间隔", "default": 1000, "min": 100, "max": 10000 },
  { "key": "tasks",        "type": "tasklist",    "label": "任务列表", "default": ["示例任务"] }
]
```

支持的 `type` 与值格式：

| type | 控件 | 值格式 | 说明 |
|------|------|--------|------|
| `text` | 短文本输入 | `"字符串"` | ≤256 字符 |
| `longtext` | 长文本输入 | `"字符串"` | 多行，≤4000 字符 |
| `password` | 密码输入（掩码显示） | `"字符串"` | ≤256 字符；值不注入页面，读取方式见表下说明 |
| `time` | 时间选择（24h，精确到秒） | `"HH:MM"` 或 `"HH:MM:SS"` | |
| `date` | 日期选择 | `"YYYY-MM-DD"` | |
| `datetime` | 日期时间选择 | `"YYYY-MM-DD HH:MM:SS"` | 空串 = 未设置 |
| `boolean` | 单选开关 | `true / false` | |
| `multiselect` | 多选开关组 | `["a","b"]` | 需 `options`，值为选中项子集 |
| `radio` | 互斥开关组 | `"a"` | 需 `options`，组内只选一个 |
| `weekdays` | 星期选择 | `["mon","wed"]` | 周一至周日多选，固定选项 |
| `select` | 下拉选择 | `"a"` | 需 `options` |
| `font` | 字体选择 | `"Microsoft YaHei UI"` | 枚举系统已安装字体，空串 = 默认 |
| `palette` | 调色板 | `"#rrggbb"` 或 `"#rrggbbaa"` | `options` 作预设色（可省略，含屏幕吸管取色与透明度滑块） |
| `number` | 数字输入 | `数字` | 可选 `min` / `max` / `step` |
| `slider` | 滑动条 | `数字` | 可选 `min` / `max` / `step`，缺省 0/100/1 |
| `timerange` | 时间范围（精确到秒） | `{ "start": "YYYY-MM-DD HH:MM:SS", "end": "..." }` | 空串表示未设置 |
| `tasklist` | 任务列表（增删改） | `["条目1","条目2"]` | |
| `todolist` | 待办任务列表（勾选） | `[{ "text": "...", "done": true }]` | 皮肤可经 `skin_set_setting` 写回 |
| `datetasklist` | 日期任务列表 | `[{ "time": "YYYY-MM-DD HH:MM:SS", "text": "..." }]` | 每条任务带日期时间，time 可空 |

`password` 类型的值**不随 `__DESK_PP__.settings` 烘焙进页面**（skin:// 全皮肤同源，注入页面会被其他皮肤抓取）；皮肤内改用 `skin_get_setting` 命令按需读取——`await __DESK_PP__.invoke('skin_get_setting', { key: 'my_key' })`，身份取自调用窗口，只能读到自己的值。

`options` 的 `label` 均可省略，回退显示 `value`。每个设置项还可用 `"description"` 加一句说明，显示在控件标签下方：

```json
{ "key": "level", "type": "slider", "label": "强度", "description": "0 到 100，影响粒子数量", "default": 60 }
```

### 分组

设置项可用 `"group"` 指定分组名，「皮肤设置」页会把同组控件归到一张卡片里（与「窗口」页的分节样式一致）。组按首次出现的顺序排列，未指定 `group` 的控件归入最前面的无标题卡片：

```json
"settings": [
  { "key": "title", "type": "text", "label": "标题", "group": "文本", "default": "Hello" },
  { "key": "notes", "type": "longtext", "label": "备注", "group": "文本", "default": "" },
  { "key": "accent_color", "type": "palette", "label": "主题色", "group": "外观", "default": "#ff3333" }
]
```

皮肤内读取与监听：

```js
// 初始值：页面加载前已由注入桥烘焙好
const settings = window.__DESK_PP__?.settings || {};
console.log(settings.accent_color);

// 运行时变更：管理器里改设置后实时推送，无需重载
document.addEventListener('desk-setting-changed', (e) => {
  const { key, value } = e.detail;
  // 应用新值……
});
```

参考示例：`examples/controls-demo`（全部 20 种控件演示，界面语言跟随管理器）。

### 安装皮肤

1. 管理器点击「+ 添加皮肤」，选择 `.dskin` 皮肤包（更新时保留用户设置数据）。
2. 安装版可直接**双击 `.dskin` 文件**：唤起 Driftlet 并弹出安装引导页，确认后安装（文件关联由安装程序注册，绿色 exe 无此入口）。
3. 开发期可直接把皮肤文件夹复制到 `<安装目录>\skins\`。

`.dskin` 就是把皮肤文件夹打成 zip（`skin.json` 在根目录且声明 `id`/`version`）后改扩展名；皮肤作者可用独立打包工具 `tools/pack-skin.exe`（免安装，无需 Node.js / Rust 环境）一键生成并校验：

```
tools\pack-skin.exe <皮肤文件夹> [输出目录]
```

详见 `docs/皮肤开发指南.md` 第 8 节。

---

## 安全模型

第三方皮肤 = **能联网的本机代码**（完整 Chromium 网页 + 后端命令调用），请按此信任模型对待。Driftlet 的防线：

- **权限声明**：注册表、Shell、系统控制（音量/媒体/打开外部/通知）、剪贴板、麦克风共 5 种敏感能力，皮肤必须在 `skin.json` 声明 `permissions` 才可调用，后端逐命令强制校验；安装引导页逐条展示声明并按两档分级标注（高危 `shell` / `system` 标红，中危 `registry` / `clipboard` / `mic` 标黄）。
- **管理器命令仅管理器窗口可调**：加载/卸载/设置等全部管理命令校验调用方窗口身份，皮肤窗口调用一律拒绝（仅拖动、边框缩放、右键菜单三个无害命令例外）。
- **皮肤窗口零授权**：皮肤窗口的 capabilities 为空，不授予任何 Tauri 核心/插件权限，只能经注入桥 `__DESK_PP__.invoke` 调后端命令。
- **文件沙箱**：皮肤的文件读写限定在自身目录内（拒绝绝对路径与 `..` 逃逸），`skin.json` / `settings.json` 禁写禁删。
- **设置值跨皮肤隔离**：`settings.json` 被 skin:// 协议拦截（含 8.3 短名、ADS 等绕过手段），A 皮肤读不到 B 皮肤的设置；`password` 类型值不落页面，由 `skin_get_setting` 按窗口身份下发。
- **`.dskin` 安装防护**：解压防 zip-slip 与 zip 炸弹（按实际解压字节计量），体积/文件数上限 64MB / 256MB / 5000；staging 回滚式安装，失败不破坏旧版本。

---

## 运行时数据位置

所有数据都随安装目录走（便携模式）：

- 皮肤目录：`<安装目录>\skins\`
- 全局配置：`<安装目录>\config\config.json`（窗口页数据与全局设置）
- 皮肤设置值：`<安装目录>\skins\<皮肤id>\settings.json`（「皮肤设置」页的用户值，随皮肤文件夹走，更新皮肤时保留）

说明：旧版本存放在 `%APPDATA%\com.driftlet.app\` 的配置会在首次启动时自动迁移；安装目录不可写（如受保护的 Program Files）时回退到 `%APPDATA%\com.driftlet.app\`。

---

## 注意事项

- "窗口置顶" / "贴在桌面" 二选一，始终启用其中一种，默认 "贴在桌面"。
- 皮肤窗口采用自定义 Win32 子类保持无边框，改动窗口 / 桌面层级相关代码前请先阅读 `docs/关键机制.md`。
- 皮肤资源统一通过 `skin://` 自定义协议加载，外部文件引用请放在皮肤文件夹内。

---

## 开发/构建命令

```bash
npm run dev           # 仅启动 Vite 前端
npm run build         # 构建前端到 dist/
npm run tauri dev     # 开发模式（前端 + Tauri）
npm run tauri build   # 生产构建安装包
```

安装包产物（仅 NSIS，`bundle.targets = ["nsis"]`；安装器中英双语自动跟随系统 UI 语言）：

- NSIS：`src-tauri/target/release/bundle/nsis/Driftlet_<版本>_x64-setup.exe`

说明：`nsis.languages = ["English", "SimpChinese"]`——运行时按系统语言自动匹配，无匹配回退数组**首位**，故 English 必须在前（中文系统 → 简体中文，其余 → English）。曾同时产出 MSI，现已不再生成。

后端单独检查：

```bash
cd src-tauri
cargo check
cargo build --release
```
