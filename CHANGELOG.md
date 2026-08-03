# 更新日志

本文件记录 Driftlet 的所有重要变更。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)。

## [1.0.2] - 2026-08-03

### 新增

- 皮肤开发热重载（仅 debug 构建）：监视 skins 目录，已加载皮肤的文件变更防抖 300ms 后自动重载（编辑器 tmp+rename 原子保存由「认最终文件名 + 防抖」吸收）；应用自身写入（`settings.json` 及其 tmp/bak、`preview.png`、`.staging-*` / `*.old` 暂存目录）一律过滤防死循环——新增皮肤目录写入点时必须同步扩充 `hotreload.rs` 的过滤清单。设置页有总开关（默认开，正常用户无需开启；皮肤运行时数据文件（如清单数据）也会触发重载，受影响时关掉即可），开关只门控重载动作、watcher 常驻即开即用。
- 布局备份导出/导入（设置页「备份」行）：导出把 `config/` + `skins/`（含各皮肤 `settings.json` 用户值）打成带清单的 zip；导入经体积/条目/zip-slip 校验与 `config/config.json` + 清单 format 验证后，卸载全部皮肤、暂存替换两个数据目录（任一步失败整体回滚）、重建运行时状态（内存配置、语言与托盘、自启动、全局热键）并按备份加载皮肤，前端随后整页重载完成再同步。

### 修复

- 装机环境系统通知整体失效（开发机正常）：NSIS 安装器预建的开始菜单 `Driftlet.lnk` 盖的是 bundle id `com.driftlet.app`，运行时自检只看快捷方式目标、误判「已就绪」而跳过重写，`CreateToastNotifierWithId("Driftlet")` 无注册快捷方式 → `Show` 成功但系统静默不显示。自检改为「目标 + AUMID 属性」双查，不匹配即重写，已装坏的用户机在下次启动时自愈。
- 设置面板打开时主题按钮等行内容随入场动画位移：面板级 transform 入场都会带动行内容——位移版（`popIn` 的 `translateY`）让整排按钮滑动 ~10px，缩放版绕面板中心进行、偏心内容仍被带 ~2.5px——设置面板改为不加面板级入场动画，仅随遮罩淡入（时长 0.25s）；确认框 `popIn` 不变。
- 矮窗口下设置/确认弹层内容被裁顶且滚不到：flex 居中遇超高面板会从顶部裁剪，遮罩加 `overflow-y:auto`、面板改 `margin:auto` 安全居中——高度足够时居中，超高时从顶部起可滚动。

### 变更

- 皮肤可按元素粒度接管右键：皮肤在 `contextmenu` 上调用 `preventDefault()` 后，宿主不再弹「打开配置 / 刷新 / 卸载」原生菜单（桥改在 window 冒泡末端检查 `defaultPrevented`，任何位置注册的页面监听器都会先生效）。此前皮肤内右键一律被宿主接管。
- 默认显隐热键 `Ctrl+Alt+D` 改为 `Ctrl+Shift+Alt+D`：前者与网易云音乐等常用软件的全局热键撞车，新装用户开箱即注册失败；已有配置的持久化值不受影响，被撞车的用户在设置页手动换绑即可。

## [1.0.1] - 2026-08-01

### 修复

- 托盘图标在任务管理器（无可见窗口进程时）与 Win11 托盘拖拽图像中条纹花屏：vendor 修补 tray-icon 的 HICON 创建（上游把 1 字节/像素缓冲误传给期望 1bpp 单色位图的 AND 掩码参数），改用 `CreateIconIndirect` + 合法 1bpp 掩码 + 32bpp 预乘 DIB。
- 窗口图标的同款 AND 掩码 bug（tao 侧）：屏蔽 Tauri 默认窗口图标（不再运行时创建 HICON），任务栏/Alt+Tab/任务管理器回退到 exe 内嵌的多尺寸图标。
- 皮肤窗口内按钮/输入框点击无反应：注入桥的拖动逻辑对 `.drag-region` 内任何左键按下都进入系统移动循环并吃掉 click；现按下点落在 `button` / `input` / `select` / `textarea` / `a` / `label` / `[contenteditable]` 上时自动跳过拖动（自 1.0 潜伏，controls-demo 窗口内无按钮未暴露）。
- 皮肤设置页长说明文字挤压/溢出控件：左侧「标签+说明」单元格补收缩约束（`min-width:0`）与任意处换行；密码输入容器 `flex:1` 的 0 基线在收缩分配中塌成 0px（输入框被顶出卡片右缘、显隐按钮落位异常），改为与其他侧排控件一致的固定 160px 右栏。
- `get_gpu_info` 显存用量恒为 0 且列表混入重复 GPU：AMD 驱动下 `IDXGIAdapter3::QueryVideoMemoryInfo` 恒返 0，显存用量改用 PDH `\GPU Adapter Memory(*)\Dedicated Usage` 按适配器 LUID 取值；IddCx 虚拟显示器（向日葵 OrayIddDriver 等）会把渲染 GPU 的名字/vid/did/显存整体克隆后混入 DXGI 枚举，按性能计数器实例过滤幽灵适配器。

### 性能

- 系统信息命令不再为 CPU/内存查询常驻全量进程表（sysinfo 收窄刷新范围；进程列表按需窄化加载，不再远程读取每个进程的 PEB 环境块）。
- 音频频谱：FFT plan 静态缓存 + 采样缓冲复用 + 仅取尾部样本，消除 10–30fps 轮询下的 plan 重建与分配抖动。
- 管理器皮肤列表预览图缓存戳稳定化：不再每次刷新都全量重解码，仅重新截取或皮肤版本更新时刷新。
- 实机进程树测量：WebView2 基线约 135MB 私有内存为平台固定成本，主进程本体约 11MB。

### 变更（接口）

- 移除皮肤接口 `get_system_stats`（旧版全合一接口，1.0 起皮肤实际不可调，细粒度命令完整覆盖）与 `get_public_ip`（公网 IP 查询需经第三方服务，皮肤可自行 `fetch`）。皮肤可用命令现为 31 个。
- 同步移除 ureq 依赖与历史死代码（`pick_skin_folder` / `install_skin` 文件夹安装命令及关联死链）。

### 新增

- 接口示例皮肤三件套（`examples/`，独立 .dskin 分发）：`sys-monitor` 系统监视（零权限，§5.2 只读系统信息全家桶）、`media-hub` 媒体控制台（音量/媒体/频谱/通知）、`toolbox` 本机工具箱（剪贴板/文件/注册表/命令/设置读写）——合起来覆盖全部 31 个皮肤命令。

### 其他

- 安装/卸载程序图标换用应用 logo（nsis `installerIcon` / `uninstallerIcon` → `icons/icon.ico`）；移除 `icon.icns`（不做 macOS 打包）。
- 示例皮肤默认关闭边框拖拽缩放（`resizable: false`）。

## [1.0.0] - 2026-07-30

首个正式版。

### 新增

- 皮肤管理：安装、卸载、加载、重新加载皮肤；支持以 `.dskin` 皮肤包（zip 格式）安装/更新，更新保留用户设置数据。
- 双击 `.dskin` 文件唤起安装引导页（安装版注册文件关联），与管理器内「+ 添加皮肤」统一入口。
- 皮肤自定义设置：`skin.json` 声明配置项（19 种控件 + 分组 + 描述），管理器配置面板自动生成「皮肤设置」页。
- 窗口能力：置顶 / 贴在桌面（二选一）、锁定位置、边缘吸附、边框拖拽缩放、50%–200% 整体缩放比例、不透明度。
- 为皮肤截取预览图，皮肤列表展示缩略图。
- 托盘图标管理，主窗口关闭即隐藏到托盘；开机自启；暗/亮主题切换。
- 全局快捷键一键隐藏/显示已加载皮肤（默认 Ctrl+Alt+D，可修改或禁用），托盘菜单同步勾选项。
- 中英双语界面：安装器语言跟随系统，应用首启语言与安装器同步，可手动切换。
- 注入桥提供管理器界面语言（`__DESK_PP__.language` + `desk-language-changed` 事件），皮肤界面可跟随管理器切换语言。
- 便携模式：皮肤与配置全部随安装目录走，旧版 `%APPDATA%` 数据首次启动自动迁移。
- 皮肤权限模型：文件 / 注册表 / Shell / 系统控制 / 剪贴板 / 麦克风共 6 种敏感能力需在 `skin.json` 声明 `permissions`，安装引导页逐条展示并标注高危项。
- 皮肤后端接口（skin_api）：系统信息、磁盘/GPU、音频频谱、音量与媒体控制、电池、剪贴板、Toast 通知等命令，敏感命令按声明逐条校验。
- 示例皮肤 controls-demo 1.0：全部 19 种控件演示，界面语言跟随管理器（随仓库提供参考实现 `examples/`，以独立 .dskin 分发，不随安装包打包）。

### 安全

- 全部约 40 个管理器命令统一 `require_manager` 校验调用窗口身份，皮肤窗口调用一律拒绝（仅拖动 / 边框缩放 / 右键菜单三个无害命令例外）。
- capabilities 按窗口拆分：主窗口仅保留实际用到的核心权限，皮肤窗口权限为空（不授予 shell / dialog / autostart 等任何核心与插件权限）。
- 修复皮肤列表中皮肤名的 XSS（HTML 属性转义 + 内联事件处理清零），收紧管理器 CSP（script-src 去掉 unsafe-inline、object-src 'none'、base-uri 'self'）。
- `password` 类型设置值不再烘焙进皮肤页面（skin:// 全皮肤同源可被抓取），改由 `skin_get_setting` 命令按窗口身份下发；设置变更事件定向发管理器窗口，不再广播。
- 删除 assetProtocol（其 scope 会把 `settings.json` 暴露给所有窗口），预览图改走 `skin://` 协议直出。
- `open_external` 收紧：可执行扩展名黑名单 23 项（exe / bat / cmd / ps1 / msi / lnk 等）、拒绝 UNC 路径、URL 与路径统一报错。
- `settings.json` 拦截双重化（canonicalize 后按真实文件名复查，防 8.3 短名 `SETTIN~1.JSO` 绕过）；路径分量含 `:` 一律拒绝（防 NTFS ADS `skin.json::$DATA`）。
- zip 炸弹防护按实际解压字节计量（不信 zip 头声明的体积），保留 64MB / 256MB / 5000 文件上限。
- 安装流程改 staging 回滚：任何一步失败不破坏旧版本、不留半成品目录；皮肤扫描跳过点开头目录。
- 加载器加固：`skin.json` ≤1MB；入口拒绝 `..` / `\` / `:`；id 黑名单 Windows 保留设备名（con / nul / com1-9 等）；目录复制跳过符号链接并限深 32 层。
- 安装引导页逐条展示皮肤的权限声明，Shell / 麦克风标高危。

### 修复

- 多个皮肤贴桌面时 z 序相互翻转：就位判定改为「紧下方是图标宿主或另一贴桌面皮肤」，收敛后叠成一摞；宿主在 z 序顶端时回退 HWND_TOP；修复前校验窗口进程归属，hwnd 被系统复用时绝不动手。
- 取消贴桌面后皮肤误出现在任务栏 / Alt+Tab（不再误加 WS_EX_APPWINDOW，两个 bit 都清理）。
- 托盘创建失败时，主窗口关闭按钮降级为直接退出，不再留下无窗口的残留进程。
- 全局快捷键被占用注册失败后，重新输入同一组合可正常重试。
- FFI 回调加 catch_unwind（皮肤窗口子类、显示器枚举），panic 不再穿越 FFI 边界；修复进程句柄泄漏；WM_GETMINMAXINFO 先默认处理再覆写。
- 全域 Mutex 中毒后连环 panic（统一改为 `into_inner()` 恢复，容忍部分状态继续服务）。
- `save_app_config` 统一归一化：version 强制写当前版本、language 与内存态同步、置顶/贴桌面模式位与配置加载复用同一实现。
- 便携迁移现在包含 skins 文件夹；安装目录可写性探测更准确；启动致命错误改为 MessageBox 提示后退出。
- 前端：皮肤编辑器异步加载代际防护、联动开关失败回滚、热键录制监听泄漏、设置面板防叠开、安装向导 busy 时序。
- 示例皮肤 controls-demo：注入桥防御性判断，资源改相对路径，适配 password 类型（经 skin_get_setting 读取）。

### 工具

- pack-skin 打包工具：与安装端同一套 `SkinManifest` 强类型校验；排除 `.git` / `.svn` / `node_modules` / `*.dskin`；缺 version 时警告；体积/文件数上限 64MB / 256MB / 5000；重构建后约 316 KB。
