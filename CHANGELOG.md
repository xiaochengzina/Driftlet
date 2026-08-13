# 更新日志

本文件记录 Driftlet 的所有重要变更。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)。

## [1.0.6] - 2026-08-13

### 新增

- **日志窗口**（设置页「高级」页签新增「日志」行 →「打开日志窗口」，按钮与备份行同款分段控件样式，打开成功后设置页自动关闭）：独立无边框小窗（label `log`，vite 多页构建第二入口 `log.html`），集中查看后端警告/报错与皮肤控制台输出。后端新模块 `app_log.rs`：自定义 `log` crate logger 零侵入捕获全项目既有打点（口径 = target 以 `driftlet_lib` 开头，wry/tao 等第三方 crate 过滤；**Warn 及以上进缓冲**——Info 级操作流水只 eprintln 到 stderr 顶替 env_logger 保留开发期可见性，实测对用户无价值、不进窗口），写入内存环形缓冲——上限 1000 条、满后淘汰最旧、单条消息截 1000 字符、每条带单调 `seq`；**仅日志窗口存在时** `push` 才 `emit_to("log", "app-log-added")` 增量推送，窗口不开前端零内存开销（后端 1000 条短字符串常驻，约数百 KB）。窗口打开时前端先 `listen` 再调 `get_app_log` 拉全量快照，按 `seq` 去重合并后转纯增量；展示按时间正序、最新在底部，贴近底部时才跟随滚动。操作流水打点（启动/托盘/退出、快捷键触发、皮肤加载/卸载、设置项修改——只记 key 不记 value 防 password 泄露）保留 `log::info!` 但仅到 stderr：实测这类记录对用户无价值（刚操作完就知道的事），日志窗口的后端内容只剩补偿错误、协议读取失败等 warn/error 级真实异常。前端 `log.js`：级别过滤（信息/警告/错误开关）+ 来源过滤 + 清空按钮（`clear_app_log`）；主题与语言由后端烘焙进 URL query（日志窗非管理器窗调不动 `get_app_config`，`initI18n` 因此加可选 forceLang 参数），运行期切换语言/主题经 `app-log-language`/`app-log-theme` 事件同步重绘。新命令 `open_log_window`（`require_manager`，已开着则提到前台防叠开；**必须 async + spawn_blocking 建窗**——同步命令在主线程 WebView2 IPC 回调里就地 build() 会把主线程卡死在 wry 建窗路径上，与皮肤建窗同路径）/ `get_app_log` / `clear_app_log`（后两者新增 `require_log_window` 按 label 把关，防皮肤经注入桥读取含内部信息的缓冲）；capabilities/log.json 授予 minimize/close。关键机制双版新增「日志窗口与 app_log」章节。
- **皮肤控制台输出自动进日志 + 显式通道 `skin_log`**（均无需声明权限）：注入桥新增 console hook——包装 `console.log/info/debug/warn/error`（先调原始方法再转发，级别映射 info/warn/error），并捕获未捕获脚本异常（带文件：行号）、未处理 Promise rejection、资源加载失败（img/script/link，`error` 事件 capture 阶段）、CSP 拦截（`securitypolicyviolation`）；桥接侧洪泛保护——队列每 250ms 整批一次 invoke（新批量命令 `skin_console_log`）、相邻重复合并 `(xN)`、每 flush 上限 30 条、队列硬顶 300 条、单条预截 1200 字符、溢出合成一条 warn 提示，hook 内部异常静默吞防递归，注入脚本保持纯 ASCII（皮肤页 charset 不可控）。`skin_console_log` 身份取自窗口 label（不走 `caller_skin` 的全量扫盘——持续高频通道每批扫盘不值），每批 ≤60 条兜底截断。显式 `skin_log` 保留：`__DESK_PP__.invoke('skin_log', { level, message })` 记业务事件（`level` 只认 `"warn"`/`"error"`，缺省 info）。两者 source 均自动带皮肤 id（`skin:<id>`）经窗口身份识别、不可伪造。前端来源过滤相应改为动态按皮肤列表（随条目出现重建选项，清空不动过滤选择）。双版开发指南 §5.4 改写（自动转发在前、显式通道在后）、§7.3 排错清单更新，关键机制双版同步。
- **「皮肤热重载」更名「开发模式」，开启后解锁皮肤 DevTools**（设置页「高级」原开关，全构建生效）：皮肤窗口内按 F12 或 Ctrl+Shift+I 打开 DevTools——注入桥 keydown（capture 阶段注册防页面吞键；浏览器加速键禁用后这两个键会以 DOM 事件到达页面）捕获后经新命令 `open_skin_devtools` 调 `ICoreWebView2::OpenDevToolsWindow` 精确开锁，后端校验皮肤窗口身份 + 开发模式运行时标志（`hot_reload_enabled`，未开启静默 no-op——桥每次按键都发，报错无处可去）；**不开 `SetAreBrowserAcceleratorKeysEnabled(true)`**——那会连带放回 F5/Ctrl+R 刷新键，并与 5 秒维护定时器的自愈重设互踩。热重载语义不变（仅 debug 构建）。双版指南 §7.1 与 F12 相关表述、关键机制双版同步。
- **创作者 DX 改进包**：①桥新增推荐别名 `window.driftlet`（与 `__DESK_PP__` 同一对象，历史名称永久兼容）并烘焙 `hostVersion`（`CARGO_PKG_VERSION`）——皮肤可运行期做能力探测；②skin.json 新增可选 `min_host_version`：宿主版本低于它时安装向导提示「要求 Driftlet ≥ vX.Y.Z，部分功能可能不可用」（`inspect_package` 复用 `update::is_newer` 数字段比较，`PackageInfo.requires_host_version` 透传，提示不拦截；pack-skin 镜像该字段并校验格式、非法直接拒绝，已重新构建 exe）；③新增 `examples/driftlet.js`（可选封装：全部皮肤命令包成命名函数 + 设置/语言事件助手，桥缺失时 reject 明确错误方便纯浏览器离线调试）与 `examples/driftlet.d.ts`（桥、全部命令签名、返回结构与事件的类型定义，放进皮肤文件夹即有编辑器自动补全）；④开发指南双版可读性改进——新增 §1.3 五分钟快速上手（完整最小皮肤两文件复制即用）、§5 章首命令速查表（权限/用途一览）、§5.6 错误处理约定（reject 文案随界面语言不可用于分支逻辑、查询类 null/标志位 vs 动作类 reject、用 hostVersion 探测而非捕获错误）、§1.2/§2.2/§3.1 长句拆分、§2.1 补 `min_host_version` 行、§5.1 补 `hostVersion` 行、§5.5/§10 易漂移约数改非数字表述、§7.2/§8.1 补封装与校验说明；章节标题改挂推荐名（`window.__DESK_PP__` → `window.driftlet`，TOC 锚点同步）。关键机制双版桥接注入条目同步。
- **安装向导固定展示免声明能力清单**：权限区块后新增一行（`wizard.freeCapabilities`，中英双语）——无论皮肤声明什么权限，都告诉用户「无需声明即可用：只读系统信息（CPU/内存/前台窗口/正在播放/进程等）与皮肤自身目录文件读写」，零声明皮肤也对用户有正确预期。双版指南 §2.3 同步。

### 变更

- **前端收敛**（架构审查落地）：新建 `src/js/dom.js` 单一事实源——`esc`/`escAttr`/`dispName`/`dispDesc` 收编五份复制（XSS 手工防线不再多处漂移）；`confirmDialog` 工厂统一三处确认弹窗（卸载/重置/导入备份：Esc 关闭、焦点落「取消」、点遮罩关闭对齐，settings 导入确认补上原本缺失的 Esc），`update-check` 结构特殊保留自定义但复用其 `bindEsc`/`closeOnMaskClick` 工具；skin-editor 三个列表控件（tasklist/todolist/datetasklist）合并为参数化 `bindListControl`（行为逐点核对不变）；删除 `Settings.onClose` 死参数与后端 `SkinInfo.has_error/error_msg` 死字段（恒 false/None 且前端从不消费）。
- **工程化补强**：新增根 `AGENTS.md`（文档双版同步、pack-skin 镜像同步+exe 重建、版本号四处一致、命令闸门、vendored 补丁保留、行尾、生成物入库七条硬约定成文）；新增 GitHub Actions 最简 CI（windows-latest：npm build + cargo test，build.rs 的清单资源要求 Windows runner）；新增 `.gitattributes`（`* text=auto eol=lf`、`*.ps1` 显式 CRLF）——`src-tauri/Cargo.toml` 长期幻影脏文件与全仓 EOL 随克隆者配置漂移的问题就此根除；pack-skin 镜像回指注释补到 types.rs/loader.rs/package.rs（主 crate 侧此前零提示）；移除 `env_logger` 死依赖（log crate 补 `std` feature）。

### 修复

- **任务栏按钮/悬停预览标题栏/Alt+Tab 显示 Windows 默认程序图标而非 Logo**：tao 建窗默认不给窗口注册 HICON，而 exe 资源图标的回退并不覆盖这些消费位；tauri 自带的 `default_window_icon`/`set_icon` 又走 tao 的 RGBA→HICON 重建路径（AND mask 缓冲 1 字节/像素 vs CreateIcon 期望 1bpp，vendored tray-icon 修过同款 bug，lib.rs NOTE 因此早已禁用）——新增 `apply_window_icon`：自解析打包进二进制的多尺寸 icon.ico 目录挑最贴合条目（不用 LookupIconIdFromDirectoryEx——其返回值是资源 ID 语义，对文件版目录是 dwImageOffset 低 16 位，索引化直接越界）+ `CreateIconFromResourceEx` 建 HICON（掩码/尺寸都正确），ICON_SMALL+ICON_BIG 双设、按窗口 DPI 取 `GetSystemMetricsForDpi` 尺寸，两枚 HICON 进程级缓存复用（日志窗反复开关不泄漏）；管理器窗与日志窗建窗后调用，皮肤窗 `skip_taskbar` + 无边框无展示位不处理。

- **全量审查修复批**：①生产构建不再探测 CWD 的 `examples/` 目录（`copy_example_skins` 仅 debug 构建生效——原注释声称生产直接返回但无门控，快捷方式启动 CWD 不可控，撞上同名 `examples/` 会静默误装皮肤）；②皮肤右键菜单加 `SKIN_MENU_OPEN` 重入守卫、模态等待挪 spawn_blocking——原 `rx.recv()` 裸阻塞 async worker，皮肤循环调用可停满 worker 池（宿主命令瘫痪），嵌套模态在主线程栈上递归；菜单开着时后续调用直接丢弃；③竞态补锁：托盘「重载全部」纳入 `install_lock`（原可与备份导入的目录替换互踩），包安装/备份导入的目录替换段持 `settings_lock`（防设置写进刚被替换的旧目录而丢失），锁序 install_lock → settings_lock 成文于 AppState；④备份导入 Phase 3 失败补「皮肤已卸载」提示（与 Phase 2 同口径，此前只报通用错误，用户不知道该重新加载）；⑤`export_backup` 的 thread::scope+block_on 取锁 hack 与 panic 点拆除（改为调用方异步持 guard，函数改收目录参数）；⑥重 IO 统一挪出 async worker（包安装/备份导入导出/预览截图/右键菜单/删除皮肤）；⑦`open_external` 黑名单补 `search-ms`/`library-ms`/`application`/`appref-ms`/`diagcab`/`website`（Explorer 搜索/库、ClickOnce 等可间接指向远程共享，与 UNC 同属 NTLM 外泄面）；⑧日志窗：级别 chip 在语言切换重绘后与实际过滤状态脱节（模板硬编码 active 改按 filters 输出）、文档级监听随语言切换逐次叠加（挪 boot 只注册一次）；⑨`set_theme` 加白名单（非法值归一 auto），自启/主题/热重载/更新检测/语言/快捷键六个设置命令的 `save_config` 错误统一包 i18n（ConfigSaveFailed）；⑩热键注销失败留痕（原静默吞，极端情况旧组合泄漏到重启）、`create_tray` 返回类型对齐全局 String 约定、`capture_skin_preview` 变量遮蔽与非 Windows 未用警告修正、`lib.rs` 管理器窗加固过期注释修正、`run()` 的 expect 改 `fatal_startup_error` 弹框（release 无控制台不再静默退出）；⑪测试补强：skin:// 路径解析抽纯函数 `resolve_skin_file`（另拆 `decode_uri_path`）并补逃逸/拦截用例、包安装补 zip-slip 条目用例——此前这两个安全不变量恰好零覆盖。关键机制双版（右键菜单守卫、open_external 黑名单、install_lock 覆盖与锁序）与双版指南（§2.3 免声明清单、§5.3 黑名单清单）同步。

- **`cargo test` 测试 exe 启动即失败（0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND，Win10 21H2 实测）**：vendored tauri-runtime-wry 消息框静态导入 `TaskDialogIndirect`（comctl32 v6 导出），而 tauri_build 的 winres 清单只经 `rustc-link-arg-bins` 嵌进 bins——lib 单元测试二进制无清单、无 v6 激活上下文，`comctl32.dll` 解析到 System32 v5（无该导出）加载期绑定失败（embed-resource 3.x 的 `rustc-link-arg-tests` 只认 tests/ 集成测试目标，本 crate 没有，无法救场）。build.rs 改 `try_build(Attributes::new().windows_attributes(WindowsAttributes::new_without_app_manifest()))` 关掉 tauri 的 bins 清单，再由 `embed_resource::compile_for_everything` 把 `windows/app-manifest.rc`（内容与 tauri 默认清单相同的 comctl32 v6 依赖）嵌进全部链接目标——bins 与测试二进制各一份不重复（build-dependency 新增 embed-resource 3）。诊断走自写 `tools/check_imports.py`（PE 导入表静态核对）；cargo test 106 过、cargo build 过、主 exe 单清单含 v6 验证过。关键机制双版新增「cargo test 的 comctl32 v6 清单」章节。

## [1.0.5] - 2026-08-09

### 新增

- **安装包完结页「开机自动启动」勾选框**（NSIS 安装引导，与其它软件同款）：MUI2 完结页原生仅两个勾选项槽位（「运行 Driftlet」与被借用为「创建桌面快捷方式」的 SHOWREADME），第三个经页面 SHOW 回调手工创建（`FinishPageShow`，坐标沿用 MUI2 Finish.nsh 勾选项间距公式 TEXT_BOTTOM 85→RUN 90→README 110→本框 130）；LEAVE 回调按勾选态写/删注册表，口径完全镜像 `auto-launch::enable()/disable()`——勾选写 `HKCU\...\Run\driftlet` + `StartupApproved\Run` 的 enabled 二进制（任务管理器「启动」页与应用 `is_enabled()` 同读，设置面板开关看到的即此处所选），不勾删 Run 值（幂等）。初值读注册表现状（重装且已开则默认勾选，全新安装默认不勾），Back→Next 恢复用户选择；更新/被动/静默安装跳过不动注册表。标签中英双语（模板内 `LangString autostart`），为 installer.nsi 继「删除应用数据复选框移除」后的第二处自定义（关键机制双版同步，含 cli 升级重打模板警告更新）。

- **权限声明新增「中危」分级**：安装引导页权限由「普通 + 高危（红）」两态改为两档分级标注——高危（红色徽标）`shell` / `system`，中危（黄色徽标）`registry` / `clipboard` / `mic`；相对旧版：`mic` 高危→中危，`system` 无标注→高危，`registry` / `clipboard` 无标注→中危。分级仅是引导页展示层，后端 `require_perm` 仍为「声明/未声明」二元校验。新增 `--warning` / `--warning-soft` 主题变量（浅/深色）与 `wizard.permMediumRisk` 文案键（中英）。
- **皮肤窗口 Alt+F4 = 隐藏而非关闭**：Alt+F4 是系统级窗口消息（WM_CLOSE），JS 层与加速键开关都管不到，tao 转为 `CloseRequested`——皮肤窗建窗的 `on_window_event` 新增分支：未登记的关闭请求 `prevent_close` + `hide()` + `hotkey::sync_tray_toggle_item` 同步托盘勾选项；唤回复用既有全局快捷键（默认 Ctrl+Shift+Alt+D）与托盘勾选项。程序化关闭（卸载/重载/退出，全汇聚 `close_skin_window_nowait`）在 `close()` 前把 label 登记进 `INTENTIONAL_CLOSES` 静态集合，事件处理消费放行，`close()` 失败撤销登记，`create_skin_window` 建窗时再防御性清理同 label 残留登记（极端场景：`close()` 入队后窗口被外部销毁、`CloseRequested` 未触发导致登记残留，不清则新窗首次用户 Alt+F4 被误放行）——vendored tauri-runtime-wry 中 `close()` 与用户 Alt+F4 同走 `on_close_requested`，`CloseRequested` 不带关闭原因，登记集合是唯一判别；`hwnd_dead` 分支走 `destroy()` 不触发 `CloseRequested`，无需登记。管理器窗口 Alt+F4 由既有「关闭=隐藏到托盘」覆盖，行为不变。
- **浏览器加速键统一禁用**（管理器 + 所有皮肤窗口）：F5 / Ctrl+R / Ctrl+F5 刷新、Ctrl+P 打印、Alt+Home、F12 devtools 等全部失效（Ctrl+C/V 等编辑键为 DOM 层快捷键不受影响）——页面不可经按键刷新/导航，窗口生命周期完全归管理器。实现 = `factory::disable_browser_accelerator_keys`（`ICoreWebView2Settings3::SetAreBrowserAcceleratorKeysEnabled(false)`）；原 `spawn_context_menu_disable_retry` 更名 `spawn_webview_hardening_retry`，右键菜单禁用与加速键禁用一起在 WebView2 异步初始化期间重试约 6 秒；此后皮肤窗与管理器窗均由 5 秒维护定时器持续自愈重设。
- **第 20 种设置控件 `stepper`（数字步进器）**：−/＋ 按钮按 `step` 增减数值，`min` / `max` 可选夹取（缺省无界，`step` 缺省 1），到界自动禁用对应按钮；小数位跟随 `step` 防浮点尾巴，点击即保存。全链路：`SkinSettingKind::Stepper`（pack-skin 镜像枚举同步），值校验/钳制并入 `validate_custom_setting` 与 `effective_settings` 的 number 臂；配置面板渲染为 `form-row` 右侧连体小组件（`.cfg-stepper`，沿用 segments 视觉语言，按压反馈进统一 scale 清单），新增 `editor.stepDecrease` / `editor.stepIncrease` 文案键（中英）。controls-demo 示例（含演示页新增的步进器驱动小秒表——改值即时重排间隔）与双版开发指南 §4.2、README 双版控件计数（19→20）同步。

### 移除

- **`files` 权限取消**：皮肤目录内文件读写（`skin_read_file` / `skin_write_file` / `skin_list_dir` / `skin_delete_file`）不再需要声明——fs 沙箱本就把一切操作限制在皮肤自身安装目录（绝对路径与 `..` 拒绝、canonicalize 包含性校验、防符号链接逃逸、`skin.json`/`settings.json*` 只读保护），沙箱即边界。后端拆 `PERM_FILES` 常量与 4 处 `require_perm` 闸，fs 命令改走 `caller_skin`（仅确认调用者身份、未安装皮肤快速失败）；旧皮肤残留的 `"files"` 声明无害（未知名忽略规则不变），安装引导页静默略过。文档（中英开发指南 §2.3/安全模型/示例清单、关键机制双版、README 双版、路线图）与 toolbox 示例权限同步。

## [1.0.4] - 2026-08-08

### 新增

- **鼠标穿透恢复**（皮肤「窗口」页开关，逐皮肤、默认关）：开启后点击/滚动穿透到下层窗口或桌面，皮肤不再响应鼠标，恢复交互回管理器面板关闭。机制 = tao `set_ignore_cursor_events` 给顶层窗口置 `WS_EX_TRANSPARENT|WS_EX_LAYERED`，OS 命中测试整体跳过窗口（含 WebView2 子孙）——当年两轮失败的真正根因是自家无边框子类无条件摘这两位（5 秒自愈兜底重剥），并非组合无效；现子类按 HWND 登记（`PASSTHROUGH_HWNDS`）对穿透窗口放行两位，命门 = 先登记再置位。旧担忧「LAYERED 破 DirectComposition 渲染」在当前 WebView2 运行时不成立（同形态最小 demo 实证）；壁纸层移除后「鼠标免疫」需求由本功能承接（贴桌面 + 穿透 = 纯展示挂件）。
- **更新检测**（设置页开关，默认开）：每次启动后台比对公开仓库（github.com/xiaochengzina/Driftlet）最新 release 与当前版本（数字段比较，v 前缀/短号段归一），发现新版本则亮出管理器窗口弹提示——「前往下载」打开最新 release 页（URL 后端固定，复用 `open_target_impl` 的 ShellExecuteW 直开默认浏览器），「取消」仅关闭；勾选「不再提示更新」后取消 = 关闭更新检测并补弹「已关闭」告知（可在设置页重新开启）。网络失败/无更新一律静默不打断启动。HTTPS 走 ureq（rustls 全平台自包含，不依赖系统 OpenSSL），阻塞调用放 `spawn_blocking`（10s 超时）；前端直连被 CSP `connect-src 'self'` 拦住，故检查全程在后端。主窗口 capabilities 补 `allow-show`/`allow-unminimize`/`allow-set-focus`（亮窗用）。

### 移除

- 管理器 Win10 的 1px CSS 内描边（原生阴影已接管窗口边缘分离，描边冗余）：`is_windows_11_or_newer` 命令、invoke 注册、前端 `isWin11OrNewer` wrapper 与挂类逻辑、`html.no-native-frame #app` CSS 规则一并摘除；`parse_windows_build` 函数本体保留（皮肤接口 `get_os_info` 自用），其在 skin_api 的 pub(crate) 再导出随命令移除；关键机制中英文档同步改写。
- **壁纸层功能移除**（原「放置三态」中的第三态，皮肤 SetParent 进桌面 WorkerW/Progman 做子窗 = 桌面图标之下、壁纸之上、鼠标物理免疫）：实现依赖全套未文档化内部结构（Progman/WorkerW/DefView 类名、`0x052C` 消息、z-order band、explorer 重托管行为），Win11 24H2 已改过一次桌面渲染管线，且失败形态全是静默视觉故障（不可见/黑底/残块）只能逐版本现场验证——综合判定维护成本高于功能价值，完整摘除（`window/wallpaper.rs`、自愈巡检钩子、前端选项、命令臂）。旧配置 `wallpaper_layer: true` 由 `normalize_mode_flags` 自动迁移为贴桌面，放置归双态（置顶/贴桌面）。完整机制与病根史见 git 历史与 `tools/win32-probes/wp-*` 探针；本节下述壁纸层修复条目为本周期工作记录，随本移除一并作废。保留物：虚拟显示器 DPI 修复（下条，与壁纸层无关）、vendored tauri-runtime-wry 死句柄 `Destroyed` 补丁。

### 修复

- 管理器「窗口」页切换显示层级时整页闪一遍淡入（同皮肤数据回灌的通用病根，三处叠加各修一处）：①层级按钮成功后整页 `load()` 重绘——改选中态就地迁移到被点按钮，点当前档位变纯 no-op（失败才整页回滚）；②后端 reload（置顶→正常/重新加载/热重载）连发 `skin-unloaded` + `skin-loaded` 各触发一次编辑器重载——`onLoadStateChange` 改同皮肤 80ms 合并为一次刷新；③`.config-panel` 入场动画对同皮肤重绘也重播——`render` 加 `animate` 开关，仅换肤/首次展示播放（语言切换重绘同免）。皮肤窗口自身的销毁重建（置顶→正常的 HWND 重建）是机制所需，不在此项。
- 管理器窗口在 Win10 上没有系统原生阴影：建窗的 `shadow(false)` 是皮肤去框提交（82b0f12）顺手带上的（该提交目标仅皮肤窗口），并非产品决定——改 `shadow(true)`，经 tao `with_undecorated_shadow` 调 `DwmExtendFrameIntoClientArea`（1px 边距）启用无边框原生阴影，与其它自绘标题栏软件同款；皮肤窗口保持 `shadow(false)` 不动（桌面挂件不能有阴影）；Win11 行为不变（DWM 本就为无边框窗口画阴影/圆角/轮廓）。
- 皮肤圆角/透明区透黑底、桌面留黑色残块（GameViewer 等远程控制虚拟显示器 + Win10 高 DPI 环境，实测定位）：虚拟屏 DPI 上报不一致（`GetDpiForMonitor` 报 96、系统 DPI 实为 120），皮肤窗口建窗后被系统改派到系统 DPI，WebView2 按新窗口 DPI 重设 rasterization scale（×1.25），而 tao 按建窗 DPI 布局（`scale_factor()`=1.0）——内容按「窗口矩形×1.25」错位合成：圆角/透明区没有内容覆盖透黑底、错位部分在桌面留残块（普通/不透明/置顶窗口同样发生，与壁纸层无关）。现建窗后与亮相后各把 controller 的 rasterization scale 强制为窗口的「布局尺」（`force_webview_rasterization_scale`；`layout_scale_factor` = 物理客户区 ÷ 设计逻辑宽，取整到 0.01——tao 的 `scale_factor()` 在该路径上虚报，不能作准），两尺一致后视觉树与宿主矩形逐像素对齐；正常显示器上两者本就相等，幂等无副作用。
- Win10 壁纸层不显示，三处病根（Win10 21H2 实测定位）：①`ensure_wallpaper_workerw` 只在「宿主 == Progman」时执行，而 Win10 经典形态的图标宿主是顶层 WorkerW，壁纸从未被搬进独立窗口；②存在性检查把图标宿主（本身就是全屏可见 WorkerW）误判成壁纸窗口，永不催生；③Win10 收到 `0x052C` 后 explorer 会把 DefView 重托管进新建 WorkerW，钉入时选定的目标瞬间作废，皮肤留在旧父窗被壁纸盖住。现钉入目标按实时拓扑判别：经典形态钉进壁纸 WorkerW（Win10 的 DefView 内容盖在宿主整个子窗口带之上，钉进宿主必被盖——探针 `wp-plain-child-test.ps1` 红方块矩阵实证：钉进壁纸 WorkerW = 可见、图标在上、鼠标免疫；顶层 z 贴附方案曾评估为候选，实测 Win10 z-order band 隔离使顶层窗进不了桌面层，跨带锚点 `SetWindowPos` 静默无效，探针 `wp-xproc-anchor-test.ps1`），24H2+ 维持钉进 Progman 紧随 DefView 之下；存在性检查排除图标宿主；巡检发现形态/父窗漂移（`needs_converge`，单向收敛）即在主线程补 pin（顺带还原漂移中被摘掉的 WS_VISIBLE）。
- 壁纸层皮肤圆角/透明区域透黑底、移动后原位置留黑色残块不自愈（Win10）：壁纸 WorkerW 带 `WS_CLIPCHILDREN`，WM_PAINT 把子窗（皮肤）区域从壁纸绘制中裁掉形成父表面黑洞，且 explorer 不为挪出区域补绘（GDI 补绘全试无效）。现 pin 时**先摘该样式位再 SetParent**（顺序命门），巡检每 tick 复查（explorer 重设就再摘）——摘掉后绘制全表面，圆角透出壁纸、移动无残块。
- 管理器「窗口」页缩放比例滑块在皮肤未加载时仍可拖动并触发保存（同页其余控件均已禁用）：补齐 `disabled`。
- 管理器密码显隐按钮 hover 变色静默失效：样式引用了未定义的 `var(--text)`，改为 `var(--text-primary)`。
- 浅色主题下「已加载」徽标描边错用深色主题绿（与深绿文字/光点色系相拼）：新增 `--success-ring` 变量随主题切换。
- `datetime-local` 输入固定 160px 裁切中文环境原生控件：拆出单独定宽 205px（与 `.dt-time` 一致）。
- 全量代码审查修复（四个 P0）：①skin.json 只写 `"always_on_top": true` 产出「两真」非法放置态（`on_desktop` serde 默认 true）——当次建窗置顶+钉桌面叠加，重启后被归一化静默归贴桌面吞掉作者意图，现 manifest 解析后归一化（显式 aot 赢；与持久化配置的 desktop-wins 语境不同：持久化值无法区分显式/默认，manifest 是作者源文件）；②skin:// URL 首段用皮肤 id 而协议按文件夹名取文件，中文名文件夹直装（loader 支持 slugify 派生 id）皮肤窗口必 404、管理器预览却正常——首段改用磁盘文件夹名；③音频采集错误状态粘连：错误只在采集闲置 30s 退出时清除，持续轮询的皮肤设备热插拔后频谱永久报错——`start_stream` 成功即清错；④fs 沙箱写路径「先建目录、后做包含性校验」，经符号链接可在沙箱外创建空目录（文件本身仍被拦住）——先对最深已存在祖先 canonicalize + 包含性校验，通过后再建目录。
- 虚拟显示器路径拖动/缩放皮肤，位置尺寸逐次累积偏移：Moved/Resized 落盘换算仍按 tao `scale_factor()`（该路径虚报，渲染层已修、持久化层漏修）——改用与 `layout_scale_factor` 同源的持久化尺（`persistence_scale_factor`）。
- 「显示层级」点击当前已选中按钮即整窗重建闪烁：`set_skin_placement` 无变化调用直返，不再 reload。
- 音频采集线程 spawn 失败一次即永久失能：加线程存活标志，error 命中且线程已死时重建。
- 皮肤中文 entry/资源名经 skin:// 加载 404：协议路径未做百分号解码（wry 递交的 URI 保留编码）——先解码再做 `..`/冒号/settings 拦截与 canonicalize。
- 「拖完皮肤立即退托盘」丢最终位置/尺寸：拖动防抖落盘定时器（约 0.5–1s）随进程退出丢失——`graceful_exit` 前同步 flush（`flush_pending_drag_saves`，无 pending 零开销）。
- 皮肤经 `skin_write_file`/`skin_delete_file` 的自写触发自身热重载（toolbox 保存即整皮 reload）：文件名清单覆盖不了任意自写路径，改「最近自写集合」（规范化绝对路径 + 数秒 TTL），watcher 消耗式过滤。
- 备份导出与导入/安装并发可产出缺皮肤的备份（导入 rename+copy 窗口期 skins/ 缺失或半拷贝，导出不持锁照样遍历）：导出路径持 `install_lock`。
- 备份导入重建后热重载开关显示与实际脱节：`rebuild_runtime` 未同步 `hot_reload_enabled` 原子镜像（双写约定漏了一处）。
- 双击 .dskin 偶发无响应：第二实例转发的 `open-skin-package` 事件在管理器 webview 未就绪时丢失——转发同时写 `pending_package`（前端 take 幂等，与冷启动兜底一致）。
- 非 UTF-8 命令行参数（lone surrogate 文件名）启动静默闪退：`env::args()` 遇非法 Unicode 即 panic 且发生在错误框就绪前——改 `args_os()` + 有损转换。
- `shell` 权限命令 `try_wait` 出错分支子进程裸跑、reader 线程阻塞泄漏（与超时分支清理不对称）：补 kill+wait+join；亚秒超时错误显示「0 秒」改 `as_secs_f64()`。
- `get_gpu_info` 同步命令跑主线程（DXGI 枚举 + PDH + 建 D3D12 设备）——改 async（对齐 get_cpu_info/get_disks_info）。
- 设置面板：主题/语言按钮选择器 `.theme-btn:not(.lang-btn)` 过度匹配（命中热键/导出/导入按钮，靠 onclick 覆盖顺序幸免）——收窄为 `[data-theme]`/`[data-lang]`；自启动/热重载开关保存失败不回滚勾选态——catch 回滚；语言切换全量重绘静默销毁打开中的安装向导（overlay 挂 #app 内被 innerHTML 重写）——改挂 document.body。
- 跨平台可编译性：`set_skin_edge_snap`/`set_skin_snap_gap` 的 `window.hwnd()` 补 cfg 门控（非 Windows 编译失败）；`default_language` 非 Windows 分支硬编码 zh-CN 改 en（与自身注释一致）；维护线程整体 cfg(windows)（非 Windows 每 5 秒空转）。
- 皮肤接口返回值批量勘正（按返回值审查逐项修复）：①`get_os_info` 的 `build` 恒 null、`is_windows_11` 恒 true——sysinfo 0.32 的 Windows `os_version` 是「{major} ({build})」无点号格式（注册表 CurrentBuildNumber 拼装），按「10.0.22631」点号三段解析从未生效，兼容两种格式重解析；`os_name` 原恒为 "Windows"，改产品名（`long_os_version` = 注册表 ProductName，如 "Windows 11 Pro"）；管理器 `is_windows_11_or_newer` 同根同病一并修（Win10 自绘 1px 边框曾误判为不需要）；②`run_command` 超长输出三条——`read_capped` 读满 1MB 即关管道读端（子进程再写吃 broken pipe 出错退出，或阻塞到超时被杀，均非文档承诺的「截断后正常返回」）改读满后丢弃后续字节直排 EOF；1MB 截断切断多字节 UTF-8 致整篇合法 UTF-8 回退 OEM 按 GBK 误码——截断回退到字符边界（仅末尾 ≤3 字节残尾才截，GBK 流不受影响）；孙进程继承管道写端时 reader `join` 无限阻塞（正常返回与超时 reject 都被拖住）——reader 改 channel 回传，超时/出错分支 detach 不 join，正常分支 2 秒宽限后按空输出放行；`try_wait` 中途出错误报「命令启动失败」改通用任务失败；`timeoutMs` 参数 u64 改 f64（小数/负数不再触发 serde 英文报错，取整后钳制 100–120000）；③`get_processes` 的 `cpu` 由单核口径（sysinfo 公式 100×Δ进程/Δ系统×核数，量程 0–100×核数，文档却写「与 CPU 同理」）除以核数归一化到整机 0–100（任务管理器同口径）；④音频采集闲置/出错退出后环形缓冲不清空，恢复轮询时回放一帧中断前旧样本——退出即清空（线程重建分支同清，panic 展开由 `AliveReset::drop` 统一清空并补记错误触发重建）；⑤`skin_write_file` 的 base64 解码失败误报「无效的路径 '\<base64\>'」——新增专用错误文案（中英）；`skin_list_dir` 单项 metadata 失败不再编造 `is_dir:false, size:0`（跳过该项，避免目录被呈现成 0 字节文件）；受保护文件名单匹配收窄到皮肤根目录直下（子目录里的同名文件原被误伤拒写），并拒绝尾点/尾空格分量（Windows 文件 API 剥尾点会让「settings.json.」落盘成 settings.json 绕过名单）；⑥`get_network_info` 的 `ips`/`local_ips` 滤掉 IPv6 链路本地地址（fe80::/10，原只滤回环）；内存 `MemoryGroup` 与 `get_disk_space` 在 total=0（swap 禁用/空光驱）时 `free_pct` 由 100 改 0（「不存在的空间全空着」语义不通）。
- `get_cpu_info` 的 `frequency_mhz` 恒为基准频率不跳动（实测恒定 1600MHz）：sysinfo 的值来自 `CallNtPowerInformation` 的 `CurrentMhz`，硬件自主 P-state（Speed Shift）的现代 Windows 上该字段不跟踪动态频率——改为任务管理器「速度」同款算法：名义频率 × PDH `\Processor Information(_Total)\% Processor Performance`（该计数器 = 实测频率占名义频率的百分比，全平台逐秒真实波动，turbo 时超 100%、显示值可超基准频率，与 TM 一致）；直读 MHz 的 `Processor Frequency` 计数器不采用（在部分平台上恒报名义值，i5-8400 台式机实测恒定 2808）。PDH 未就绪（首调基线/计数器缺失）时回退 sysinfo 静态值。
- release 打包编译失败（E0433）：`hotreload` 模块整体 `#[cfg(debug_assertions)]` 门控（release 编译期排除），而 `skin_api/fs.rs` 的 `write_file`/`delete_file` 两处 `note_self_write` 调用未同步门控——调用点补同款 cfg，debug 热重载行为不变。

### 变更

- 交互动画统一治理（规范落 `docs/交互动画.md`，源自项目本地新装的 emilkowalski skills 审计法，完整规则目录在 `.agents/skills/review-animations/STANDARDS.md`）：①缓动分治——全局唯一弱曲线 `--ease`（近 ease-in-out）原包揽入场/出场/按压全场景，新增强 ease-out 令牌 `--ease-out: cubic-bezier(0.23, 1, 0.32, 1)`，入场/出场/按压统一换用，`--ease` 收敛回悬停/颜色场景（UI 禁用 ease-in 系起步慢的曲线）；②时长收进 300ms UI 预算——toast 出场 0.45s→0.2s；③高频降载——选皮肤/切页签的内容区入场由 fadeUp（0.25s 带 6px 位移）改 contentFade（0.15s 纯透明）：列表导航属每天数十次场景，位移入场拖慢感知；④按压反馈补齐——九类按钮（win/icon/add/load/action/theme/confirm/chip/settings-close）统一 `:active scale(0.97)`，各自 transition 精确追加 `transform 0.12s var(--ease-out)`（保持无 transition:all 约定）；⑤`prefers-reduced-motion` 由「0.01ms 一刀切清零」改「降级非清零」——颜色/透明度反馈与加载 spinner 保留（可理解性），位移/缩放类入场改播纯透明帧、hover/按压 transform 关闭（文件末尾同特异性层叠覆盖，开关滑块 12px 微距位移按状态指示保留）；⑥设置页签页切换补 0.15s 纯透明淡入（display 切换即重播，与配置面板同参数）。
- 设置面板选项渐多超高滚动，改为页签布局（与皮肤编辑器 `cfg-tabs` 同一套视觉与交互模式）：通用（开机自启动、自动检测更新、全局快捷键）/ 外观（主题、语言）/ 高级（备份、皮肤热重载），页签态随语言切换重绘保留；面板高度回落到最小窗口（640×460）内，不再需要滚动。
- 管理器「窗口」页文案优化：「锁定位置」→「禁止拖动」（开关与反馈同步）、「放置位置」→「显示层级」且桌面态选项→「正常」；显示层级提示按 Pinner 实际行为重写——正常层级 = 普通程序窗口 Z 序规则（可遮挡、交互时浮起），仅「显示桌面」时不被隐藏；禁止拖动/鼠标穿透/边缘吸附提示去重复、顺语序。
- 配置页反馈降噪：各保存项与开关成功不再弹 toast（控件状态迁移即是反馈），仅失败提示；鼠标穿透开启保留警示性提示；15 个随降噪废弃的文案键（中英双语）同步清除。
- 交互补全：删除/重置确认弹窗支持 Esc 关闭、初始焦点落在「取消」、关闭摘除键盘监听；列表与配置页加载/卸载按钮点击即禁用防连点并发；开关键盘焦点可见（`focus-visible` 描边）；禁用按钮 hover 不再触发动效（统一 `:not(:disabled)` 门控）；toast、皮肤路径、版本号等文本可选中复制；窗口重显假 hover 门控（`body.hover-ok`）覆盖全部交互元素（原仅窗口按钮）。
- 样式一致性与可维护性：`--text-faint` 对比度提至 WCAG AA（小字 4.5:1）；行标签字号统一 13px；禁用透明度收编 `--disabled-opacity`；主色渐变收编 `--accent-grad`（7 处）、danger 语义白收编 `--danger-ink`；输入控件基样式与焦点环（3 组）、版本/数量徽章（2 枚）重复规则合并；删除死代码 `.confirm-note` 与 no-op 规则；`transition: all` 全量改为精确属性列表；`.confirm-hint` 三连 `!important` 改特异性实现。
- 全量代码审查变更项：①删除三个死命令及配套——`update_skin_config`（另有三处与单项 setter 不一致：缺归一化、忽略 zoom、多项改动静默不生效）、`refresh_skins`、`save_app_config`，连同 `factory::update_window_config`、前端三个 wrapper 与注册项一并摘除；②安装与加载校验口径补齐——entry 名安全规则与 skin.json 1MB 上限下沉到安装期（原仅扫描期拦截，问题包「装完即消失」），持久化类 setter（placement/click_through/edge_snap/snap_gap）补皮肤存在性校验，`remove_skin` 走 install_lock 防并发安装竞争，`reset_skin_config` 连 .bak/.tmp 一并清理，`get_skin_detail` 返回的 zoom 先钳制；③pack-skin 校验对齐安装端并重新构建 exe——补 Windows 保留设备名（con/aux/com1 等打包期即拒）、manifest 镜像补齐 5 个字段（类型错误打包期即拒）、entry 名安全规则、1MB 检查、排除清单大小写不敏感+非根目录排除提示；④文档同步——CHANGELOG 补 [1.0.3] 节（壁纸层放置新增、commit 组、核显显存修复等漏记/错放条目归位），英文版关键机制 GPU 与 tray 补丁条目翻译同步、预览截图独立成节，README 双版更新文案与功能清单（鼠标穿透），皮肤开发指南 §7.1 热重载说明，路线图/设计系统机制描述更新；⑤可维护性——扫描去重按文件夹名排序（确定性），删除死代码（Position 结构体、skins_directories 字段、common.installFailed 死 key、冗余簿记/回补、tsc 空转步骤），「未知放置模式」等用户可见错误走 i18n（含备份导入部分卸载新词条），权限清单/PDH 示例/notify 函数名等注释失真修正，gpu/audio 锁统一防中毒写法。
- `get_gpu_info` 返回值新增 `gpu_type` 字段（`"discrete"` 独显 / `"integrated"` 核显，复用 D3D12 UMA 判定，无需新建设备查询）；核显显存口径由「专用 + 共享系统内存」改为仅共享系统内存——专用段只是 BIOS 划分的一小块，「专用 + 共享」合计可能超出物理内存，总量/占用/百分比统一按共享计（核显占用本来也几乎全部落在共享计数器）。
- `get_media_info` 新增 `cover_mime` 字段（封面 MIME，按 magic bytes 嗅探 jpeg/png/gif/bmp/webp，认不出为 null）——皮肤不再盲猜 `image/jpeg`；media-hub 示例同步改用该字段。
- `get_media_info.position_secs` 文档补注快照语义（播放器上报值，不随播放推进，需平滑进度请自行插值）；`run_command` 文档补全 `code` 负值口径、timeout 下限 100ms、孙进程持管输出不全；`get_foreground_window_info` 补注 title 512 截断与 process_name 空串；`get_monitors` 补注副屏 rect 可为负坐标；`get_audio_spectrum` bands 与 `set_volume` 补注钳制行为；`read_registry_value` 补注 qword 超 2^53 丢精度；`get_os_info` 示例按实际格式勘正（`os_version: "11 (22631)"`）。

## [1.0.3] - 2026-08-05

### 新增

- 壁纸层放置（放置升三态：置顶 / 桌面 / 壁纸层）：皮肤 `SetParent` 进桌面图标宿主、z 序紧随 DefView 之下——桌面图标之下、壁纸之上，可见但三键/滚轮/悬停物理免疫；统一 `set_skin_placement` 命令替换旧双命令；explorer 重启自愈重建 + 活体 z 序复检。
- 内存信息接口新增 commit 组（虚拟内存/已提交）：psapi `GetPerformanceInfo` 取提交量与提交限制，与任务管理器同源；sys-monitor 示例与开发指南同步。

### 修复

- 壁纸层只在装了动态壁纸软件的机器上可用：原生 Win11 24H2+ 桌面的壁纸不是窗口——explorer 用 DirectComposition 把它画在 Progman 子窗口带之上，皮肤钉入后必被盖住不可见。钉入前现会检测并给 Progman 发 `0x052C` 催生接管壁纸渲染的全屏 WorkerW（壁纸搬进窗口后 z 序恢复生效），巡检顺带补发（带冷却）；动态壁纸软件此前替用户发过该消息，故开发机一直正常。
- explorer 重启后壁纸层皮肤有概率掉到桌面层级、可交互：自愈在 explorer 刚重启的窗口期（Progman/DefView 尚未重建）就触发重建，pin 找不到宿主失败、回退成普通窗口且不再重试。现自愈等宿主就绪（`host_ready()`）才重建，pin 失败一律进待重试表、宿主就绪后经主线程补钉（覆盖开机自启早于 explorer 的同型竞态）。
- explorer 重启后壁纸层皮肤无法自愈、手动加载报 `already exists`：进程被杀时窗口管理器不给跨进程子窗投递 `WM_DESTROY`，tao 的 `Destroyed` 永不触发，Tauri 注册表条目与 label 永久卡死。关闭路径检测到死句柄改走 `destroy()`，并 vendor 修补 tauri-runtime-wry：destroy 时发现死句柄补发完整 `Destroyed` 流程释放 label。
- 壁纸层 SetParent 失败残留：失败时已登记条目与 WS_CHILD 样式不回滚，巡检会把普通顶层窗反复压到桌面宿主之下致其僵尸不可见；现失败即摘登记并还原样式。
- explorer 重启后控制台报 "Error removing system tray icon"：tray-icon 的 TaskbarCreated 处理先删旧图标，而旧图标已随死掉的任务栏消失、删除必然失败，上游无条件 `eprintln!` 打出噪音日志（托盘本身经 NIM_ADD 正常恢复）；vendored crate 该路径改走安静变体（第二个 NOTE(driftlet) 补丁）。
- 核显显存占用恒 0、总量失真：核显等统一内存适配器的显存总量与占用均按「专用 + 共享系统内存」计算（任务管理器同口径——核显专用段只是 BIOS 划分的一小块，只取专用会得到占用恒 0、总量失真）；核显判定走 D3D12 UMA 架构查询（`D3D12_FEATURE_DATA_ARCHITECTURE.UMA`，结果按 LUID 缓存），不按专用显存大小猜（APU 可在 BIOS 划出 1GB+ 专用段）。
- 备份导出/导入按钮防重入：原生文件对话框存续期间禁用按钮，重复点击不再叠开多个选择器；导入确认框同步防叠开。

### 变更

- 皮肤热重载默认关闭：皮肤作者开发时在设置页自行开启；备份文案去「布局」——文件选择器过滤器与导入弹窗标题由「Driftlet 布局备份」改为「Driftlet 备份」，弹窗红字警告改为「未在备份中的皮肤将会被覆盖移除」。
- 管理器「窗口」页分区调整：拖拽调整大小/缩放比例/边缘吸附/吸附间距移入「位置和大小」分区；缩放提示语随位置变化改为「上方宽高」。
- 管理器配置行距调宽：表单行上下内边距 8px→12px，开关等设置项不再拥挤。
- 放置三态文案收紧：「贴桌面」简化为「桌面」，hint 去掉 Win+D 并补全语义（桌面在普通窗口之下，壁纸层只显示不交互）；边缘吸附提示语去掉结尾「（不会移出屏幕）」（中英同步）。

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
