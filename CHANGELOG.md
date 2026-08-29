# 更新日志

本文件记录 Driftlet 的所有重要变更。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)。

## [1.2.1] - 2026-08-29

### 新增

- **安装器体验重写（文案去翻译腔 + 用户协议许可页 + 品牌文本显示版本号）**：`installer.nsi` 自定义 LangString 全量换血（中英双语）——欢迎页删「必须关闭其它程序」等模板废话与否定式改写；重装页升级/降级/同版本三分支推荐语与选项标签全部自有文案：保留数据类选项标注「保留现有数据」、卸载类标注「数据将被清除」（普通卸载必删数据，POSTUNINSTALL 钩子口径）；完结页「开机自启动」勾选措辞与设置页开关（i18n）逐字一致，兑现模板注释里的同步约定。**用户协议许可页**：新增 `windows/installer-license.txt`（中英双语、中文在前、九条：免费使用/免费开源禁倒卖/允许二创/二创禁盈利/第三方皮肤责任/隐私/免责/协议更新/GPL-3.0 并行）——模板内硬接线 MUI_PAGE_LICENSE（tauri cli 的 NSISConfig 无 license 配置字段，配置即校验拒绝）；协议文件须 UTF-8 带 BOM（无 BOM 时编译不报错、运行时按 ANSI 解码，许可页中文全乱码——实测发现）；路径正斜杠贯穿 JS/Handlebars/NSIS 多层渲染。许可页未滚动到底「我同意」不可选；被动参数（/P）下整页不出现。**品牌文本**：BrandingText 由恒空的 `${COPYRIGHT}`（回退 NSIS 默认 "Nullsoft Install System"）改为 `${PRODUCTNAME} ${VERSION}`——各页右下角显示 Driftlet 版本号且随发布自动更新。

- **竖栏亮暗主题快速切换钮**：图标栏底部（与设置钮同钉底）新增主题切换图标钮——亮模式显月亮（→点按切深）、暗模式显太阳（→点按切浅），官方 Feather moon/sun。点击按**当前生效主题**（`data-theme` 实态，auto 模式即系统解析结果）切到反方显式模式（写 `set_theme` + 立即应用）；图标与标题由 MutationObserver 盯 `data-theme` 属性自动同步——设置页三态钮改动、auto 模式下系统主题切换、本钮点击，任何来源的变化都命中。i18n 双语 2 键。

- **布局方案（保存/应用/管理命名布局）**：把当前桌面状态存为命名方案，一键切换——布局 = **加载集 + 各皮肤几何（位置/尺寸）+ 显隐**（语义边界：皮肤行为配置不归布局管，仍归各皮肤 `skin_settings` 自管；快照字段 `x/y/width/height/visible`）。三条命令（与分组同「整体写/原子捕获」哲学）：`capture_layout(name, overwrite_id?)`（原子捕获当前；`l-<毫秒>` id；覆盖保原 id 与数组位序）、`apply_layout(id)`（卸载不在方案的、写回几何后未加载按配置建窗/已加载原地更新位置与尺寸（尺寸按有效 zoom 折算实际值）、按快照显隐；返回 `{applied, skipped}`——方案里磁盘已不存在的皮肤单列提示）、`set_layouts(layouts)`（整体写：重命名/删除/排序，白名单归一）。**管理器布局面板**（侧栏 footer 布局钮 + 新模块 `layouts.js`：保存行 + 方案行（名称/皮肤数/应用/覆盖/重命名行内输入/删除确认框），复用 confirm 系弹层骨架与分组编辑同款键位语义）；**托盘「布局方案」子菜单**（每方案一项点击即应用，`apply_layout_impl` 与命令同实现；布局变更后托盘菜单自动重建）。应用成功后面板回调刷新列表与编辑器；托盘显隐勾选经 `sync_tray_toggle_item` 漏斗同步。i18n 双语同步（面板 + 报错 + 托盘菜单）。

- **从源同步副本（皮肤多开下半章）**：源皮肤更新后副本可一键追平——编辑器页眉显示「副本 · 源自 X」（双语源名），源版本与 `x-driftlet-origin` 记录不同时追加 accent 提示「源已更新到 vX」；操作分区「从源同步」钮（仅源有更新且未加载时可点）。后端 `sync_skin_copy`（ManagerOnly）：三段式重放（副本文件夹挪 `.<folder>.old` → 源文件夹拷入 → 恢复副本 `settings.json` → 重写副本 manifest 保留 id/name/name_en 身份与 `x-driftlet-origin` 新版本 → 清理让位备份，失败还原为同步前状态）；副本窗口配置与设置值全保留（源 schema 变更按安装更新同款语义兜底）。`Skin.origin` 由 loader 单独从 manifest 文本提取（不动 SkinManifest 结构，旧版打包工具零扰动）；`SkinDetail` 下发 origin 三元（原串/源显示名/源新版本）。运行中拒绝（与删除/副本同口径）；源已删除时同步钮禁用。单元测试覆盖重放语义（内容换源/身份与设置保留/origin 刷新/暂存清零）。i18n 双语 5 键。

- **托盘「皮肤显隐」子菜单**：托盘菜单在「隐藏已加载皮肤」下新增按皮肤细分的勾选项——每个已加载皮肤一行（显示名双语随语言、按名称排序），勾选 = 窗口可见，点击切换该皮肤显隐（与皮肤专属热键同一 `toggle_one_skin` 路径）。维护分两层：加载集变化才重建整个托盘菜单（`AppState.skin_vis_items` 与 `registry.loaded_ids` 比对），纯显隐翻转只按真实窗口可见性 `set_checked`（热键连发不抖菜单）；勾选态与全局热键/编辑器/右键菜单全部经 `sync_tray_toggle_item` 漏斗按真实窗口状态同步；文件夹被外部删除的僵尸窗口以 id 兜底名称防「加载集≠菜单集」恒等式误触发反复重建；无已加载皮肤时显示禁用占位行。i18n 双语 2 键。

- **皮肤专属显隐热键**：编辑器「窗口」页行为分区新增「显隐快捷键」录制钮（与设置页全局热键同款交互）——每个皮肤可配一个专属组合，按下切换该皮肤窗口显隐。配置存 `skin_settings[id].hotkey`；注册表常驻（`SKIN_HOTKEYS`：规范化组合串 → 皮肤 id，热键事件统一经 `hotkey::dispatch_hotkey` 分发、专属优先于全局），**无需加载/卸载钩子**——皮肤未加载时按下静默无效果；启动与备份导入后按 config 全量重建。冲突三重防护：与全局热键重复 / 与其他皮肤重复（i18n 新键 HotkeyConflict）/ 被其他程序占用（回滚旧组合且**不写配置**，与 set_hotkey 同款纪律）。新命令 `set_skin_hotkey`（ManagerOnly）。热键录制交互提取为 `dom.js` 共享件 `bindHotkeyCapture`（设置页与编辑器共用，单一事实源——录制中 window 级监听的摘除纪律随件携带）。i18n 双语 2 键。

- **选择性导入备份（合并模式）**：备份导入审查清单每行新增勾选——全部勾选 = 原全量替换式导入（行为不变）；取消任意勾选即转**选择性合并导入**：只替换勾选的皮肤文件夹（同 id 不同名时保本地命名；每个替换自带 `.<folder>.old` 让位备份，中途失败逐个还原——config 在目录全部就位前不动），config.json 只并入选中皮肤的 `skin_settings` 与 `loaded_skins` 成员，其余皮肤、语言/主题/自启/热键/分组一律不动（选中皮肤落「未分组」）。命令 `import_config` 加可选 `skin_ids`（空 = 全量）；返回 `{imported, skipped}`（skipped = 勾选但备份中不存在，前端单独提示）。对话框带全选/全不选、零选中禁用确认键、提示语随全选/子选切换；选择性导入成功后只刷新皮肤列表（全局项未动不整页重载）。i18n 双语 6 键；`swap_selected_skins` 两例单元测试（按 id 替换保命名 + 失败回滚）。

- **皮肤显隐控制（管理器侧首个单个皮肤显隐命令）**：编辑器「操作」分区新增「隐藏皮肤 / 显示皮肤」切换钮（仅已加载时出现，与卸载钮相邻——两功能同一坑位，按当前 hidden 状态取反）；新命令 `set_skin_visibility`（ManagerOnly），与皮肤侧 `skin_hide`/`skin_show` 同一窗口动作、同一 `sync_tray_toggle_item` 同步漏斗（托盘勾选与列表/编辑器「已隐藏」徽标按真实窗口状态刷新），只显示不抢焦点。**皮肤窗口右键菜单同步新增「隐藏皮肤」项**（位于刷新与卸载之间——原生 TrackPopupMenu 加一项，隐藏当前窗口走同一漏斗，不取生命周期锁）。i18n 双语 4+1 键。

- **组内皮肤批量控制**：组 ⋯ 菜单在「编辑分组 / 删除分组」下加分隔线，新增四项——加载/卸载/隐藏/显示组内所有皮肤。只对「目标状态之外」的成员执行（幂等：全已处于目标态时报「没有可操作的皮肤」），逐个串行调用（窗口创建不开并发），计数 toast 反馈（成功数 + 失败数分报）。i18n 双语 8 键；菜单图标全族统一为 Feather 细线条语系（加载/卸载 = download/upload 镜像对「下载到桌面/从桌面收走」、隐藏/显示 = eye-off/eye——初版自绘 play/square 因几何填充感与尺寸不一被评审替换）。

- **大卡片样式与布局重构（两版评审迭代后的最终形态）**：**名称全宽双行夹截断**（`-webkit-line-clamp: 2` + `overflow-wrap: anywhere`——47 字符无空格英文名两行完整装下），再长由 title 悬停兜底；版本芯片随行标题行右侧（身份元数据归标题区；行盒中心对齐（margin-top 2px）——选行盒不选墨色：字体度量是布局期值不随系统 DPI 缩放变，实测 100/125/150/200% 四档缩放下与名称中心偏差 ≤0.44px）；**作者不进卡片**（编辑器页眉「作者：」是其正式席位，搜索仍可按作者匹配）；footer 收敛为单行纯「状态徽标 ←→ 加载钮」两端。**名称不压预览图**——迭代中试过图底实心玻璃条与渐变 scrim 两版，实机评审结论：压在图上的任何处理都被图片内容绑架（纯白底/透明捕获的预览上深条是一块补丁），名称回卡片内容区、图片保持纯净。预览图 100px → 112px；卡片留白 8px → 10px。

- **皮肤分组管理（设计方案评审后落地）**：侧栏列表支持自定义分组——组头折叠/展开（折叠态持久化）、组头悬停 ⋯ 菜单（编辑分组 / 删除组，删除不删皮肤、成员回落未分组）、列表末尾「+ 新建分组」入口。**组的改名与成员增删统一走「分组编辑」对话框**（新建/编辑同一入口）：名称输入（必填校验留空不关闭）+ 皮肤勾选清单（勾选 = 加入本组；勾选他组皮肤即移入，行内右侧小签明示其现属组名，「移」可预期）。对话框复用 confirm 系弹层骨架（bindEsc/closeOnMaskClick 小工具 + confirm-* 视觉类，dom.js 注释指明的特殊弹窗模式），皮肤清单滚动区限高。数据两张表存 `config.json`：`skin_groups`（有序数组：id/名称/折叠态）+ `skin_group_map`（皮肤 id → 组 id），「未分组」为内置虚拟组（不落盘、恒在末尾，**只读标签行**：无 ⋯ 菜单、不可折叠、无 hover 亮底/手型光标——不伪装成可操作分组；等宽占位让它的计数与常规组垂直同轴；计数徽章透明底 + 细描边，行 hover 与否外观一致）——老配置升级零迁移；孤儿条目随 `prune_stale_entries` 清理（皮肤消失或指向已删组的归属）。读写口径：读走 `get_app_config` 整体下发，写为新命令 `set_skin_groups` 整体回写（ManagerOnly，白名单归一：组名 trim 后 1–64 字符、组数 ≤64、组 id 去重、归属表剔除空键与指向不存在组的条目），落盘失败回滚 + 弹错。搜索期间保持分组结构（无命中组整组隐藏、组头计数变「命中/总数」）；无分组时列表与特性引入前完全一致（仅末尾多一个新建入口）。i18n 双语 16 键。

- **皮肤多开（「创建副本」，方案 A 完整副本路线落地）**：编辑器「操作」分区新按钮（仅未加载时显示，与删除同口径）——后端 `duplicate_skin` 克隆皮肤文件夹为新 id（`<id>-copy` / `-copy-N`，基名截断保 ≤64）+ 新名称（`<名> 副本` / `副本 N`，bilingual 皮肤 `name_en` 同步 ` Copy` 后缀），副本 `skin.json` 写入 `x-driftlet-origin: "<源id>@<版本>"` 记录来源（SkinManifest 不拒绝未知字段，旧宿主安全；供未来「从源同步副本」）；窗口配置深拷贝继承且位置 +32px（防两窗完全重叠以为没开出来），自启清单不带入（副本默认不加载）。与安装/删除同一把 install_lock + settings_lock；运行中拒绝（先卸载，新 i18n 键 UnloadBeforeDuplicate/DuplicateSkinFailed）；改写 skin.json 失败拆除半成品目录。成功后刷新列表并选中副本（配置页联动切到副本，即可调整独立设置）。副本与「又装了一个皮肤」无法区分——设置/预览/权限/备份各自独立，零架构改动。i18n 双语同步。

### 变更

- **选择性导入提示语补齐语义说明**：审查框取消任意勾选后，提示语写明「布局方案并入，分组不导入（选中皮肤落『未分组』），全局设置不动」——分组保持不跟走是有意的设计选择，本轮只补描述不改行为；i18n 双语同步。

- **导入备份审查框勾选框移到行右侧并垂直居中（实机反馈）**：原居左且钉顶部（`margin-top: 2px`），行高两行（名称 + 权限胶囊折行）时视觉失衡；勾选框 DOM 序移到名称/权限块之后，label flex 从 `align-items: flex-start` 改 `center`，勾选框 margin 归零；滚动清单右缘补 10px 留白，勾选框与滚动条拉开距离（实机反馈贴条）。

- **分组编辑名称输入框焦点样式并入布局方案输入框一族（实机反馈）**：`.group-edit-name` 基类统一为灰面无边框 + 聚焦内嵌 1px 单环——旧式 `border + 3px accent-soft 外环` 只剩分组对话框一处未迁（布局保存/重命名输入框早前已随多轮评审迁内嵌环）；布局保存行的重复声明随基类上移摘除，分组/布局保存/布局重命名三处输入单一事实源。

- **布局方案面板 UI 重排（评审问题一次理清：行钮拥挤 + 关闭路径弱 + 无操作反馈）**：**页眉**为 display 字体 h2 眉题 + 副题（布局语义精简一句话）——页眉/页脚贴近设置页语言（图标徽章经评审撤销、关闭改设置页同款全宽钮），中间保存区与清单块维持原语言；实机评审一轮迭代：图标全部换官方 Feather 原文路径（grid/save/refresh-cw/edit-2/trash-2/check/x——手绘改动一律回退）；输入框聚焦收敛为内嵌 1px 单环（摘除全局 :focus-visible 外描边，修「双重描边」）；重命名态行不再吃悬停白底（编辑态与悬停反馈互斥）；入场动画对齐设置页大面板待遇（摘共享 popIn 的 8px 上移——体量放大后被衬成约 1 秒的向上漂移，只随遮罩淡入）；**方案行**从四枚文字钮（应用/覆盖/重命名/删除挤占行宽）收敛为「名称 + 皮肤数弱 mono 签（去盒，弱信息用弱形式）+ 应用主钮 + 覆盖/重命名/删除三枚幽灵图标钮」（Feather refresh-cw/pencil/trash 语系，悬停转语义蓝、删除悬停转红——与组菜单 danger 同源）；**保存区**名称输入 + 存档图标主钮（分区标签再评审撤销——按钮自带说明，多余）；**页脚**为「计数 + 托盘菜单发现路径」一行尾注 + 全宽 settings-close（设置页同款）；**空态**补网格图标氛围签 + 引导文案；**重命名行**在灰清单底上反白为白面输入 + 内嵌聚焦环（与保存行灰面输入同族互反），✓/✕ 并入图标钮族；**操作忙态**——应用/保存异步期按钮禁用并换「应用中…/保存中…」文案、图标钮只禁用，finally 统一还原（isConnected 判别）防连点。i18n 双语新键（subtitle/overwriteTip/footerCount/footerHint/applying/saving；saveLabel 随标签撤销移除）。纯呈现层变更：命令/数据/键位语义零改动。

- **管理器 UI 全面打磨一轮（截图评审复核）**：**设置面板独立按钮暗色隐形修复**——「检查更新」等直接坐在面板上的 action-btn 与面板同为 bg-surface，暗色下描边近隐形、整个融进面板，沉一档 bg-input 底 + 强描边，任何主题可辨、hover 仍抬升；**预览占位图标 20→24px**（112px 高画布上 20px 读感过小，不透明度 45% 规格不动）；**hover-ok 门控契约补挂四处**——`.skin-delete-btn:hover` 与 `.disabled:hover`、滑块菱形拇指 hover 主块与 reduced-motion 镜像块（触屏 tap 后 hover 滞留隐患）；颜色/时长/圆角三向审计零违例（硬编码色全在豁免清单内、时长全在令牌表、圆角均为小件语境值）。复核：截图亮暗 × 主界面/编辑器/布局面板/设置/空态 + 640×460 挤压态全绿。

- **管理器全域 UI 打磨轮（逐面截图评审：整窗编辑器/皮肤设置页/设置页/确认框/安装向导/toast/空态 × 双主题）**：① 滑块轨道 `bg-input` → accent 淡染——原轨道在白卡与深卡上都近于隐形（主控件轨道必须可辨），淡染与菱形手柄同属品牌色语言；② **危险色纪律回收**：编辑器「卸载皮肤」从 danger 红改中性——卸载完全可逆（再点加载即回），与「重置数据」红钮并排时两个红钮互相稀释，危险色此后只标不可逆操作（与皮肤卡片 unload 钮中性描边同口径）；③ 设置页全局快捷键钮补按钮感——它是全设置页唯一无 `.theme-options` 容器的裸 `theme-btn`（裸态像纯文本），改坐标牌徽章语言（描边 + accent 等宽字，与 `.brand-version`/`.sidebar-count` 同族；规则写于 `.theme-btn.active` 之前，录制中的渐变高亮态不被覆盖）。评审确认无需改：页签分段控件、hero 页眉卡、权限声明分级行、确认框、toast、空态、双主题对比度。截图逐面复验。

- **本轮新增 UI 打磨轮（评审清单过一遍，纯样式无行为变更）**：组计数从裸文字改灰底 mono 芯片（与 `.skin-card-ver` 版本芯片同族——折叠组的计数是组内容唯一规模提示）；组头 padding 5px 6px→5px 8px、组头与成员卡片间距 2px→4px（组归属的视觉节奏）；**修选中态被 hover 抹掉的一致性缺陷**——大卡片 `selected:hover` 此前被 `shadow-pop` 整体覆盖（左色条/外环在悬停选中卡时消失），改色条+外环+pop 三层 box-shadow 叠加（上浮反馈与选中标识共存）；分组编辑对话框「现属组」签去 chip 化（白 chip 在灰清单底对比过强抢戏——弱信息用弱形式，纯 mono faint 文字）；组 ⋯ 钮补 `:focus-visible` 显现（与 `.skin-delete-btn` 键盘可达处理一致）；预览框圆角同心化（10px → 6px = 卡 16px − 留白 10px，内件弧线与外卡平行，角部不「鼓」——2x 放大 A/B 定稿）；备份导入审查清单的行间分隔线 60% → 95% 宽（实机反馈，quiet 的 9% 透明度不变）；**侧栏左侧新增竖排图标栏**（44px，VS Code Activity Bar 式：布局/排列/刷新/文件夹从上到下，设置 margin-top:auto 钉底；按钮 id 不变，bindToolbar 零改动——footer 的图标工具整体迁入，footer 只剩全宽「添加皮肤」主按钮；窗口总宽不变，44px 由主面板区吸收——config-panel 居中 640px 上限，两侧 auto 边距天然吃下；**设置钮替换为 feathericons.dev 现行官方 settings 齿轮**：库内旧路径是手写近似的异版（单弧扫描齿形，与官方双弧齿形不同——14px 下齿缘毛糙「除了齿轮还有其它东西」的根因）；官方路径 + 官方 stroke 2（中间试过的 16px 放大版与两版自绘齿轮均被实机评审否决，留档决策链）；**管理器图标全量官方化**——以设置齿轮案为鉴，逐文件盘点 40 处内联 SVG，11 处手写异版全部替换为官方 Feather 路径：刷新（简笔单弧 → 官方 rotate-cw）、搜索放大镜 ×3（r=7/16.5 异版 → 官方 r=8/16.65）、拖放遮罩包裹（简笔 → 官方 package）、组菜单铅笔（缺收尾段 → 官方 edit-2）与垃圾桶（**缺内部两条竖栅** → 官方 trash-2）、空态网格（rx 异版 → 官方 grid）、搜索空态镜身（对齐官方几何）、预览占位图 ×2（polyline 山体 → 官方 image）、向导包裹图标（Bootstrap 填充盒 → Feather package 统一线稿语系）、向导对勾（坐标异版 → 官方 check）与失败叉（坐标异版 → 官方 x）。窗口 chrome（最小化/最大化/关闭）与品牌插画（漂流瓶/航迹）按设计保留自绘；**新功能 UI 打磨轮②**：布局面板重命名行补 ✓/✕ 显式操作钮——裸输入框无可见提交路径（Enter/blur 提交、Esc 取消是盲操作），并修焦点顺序坑（blur 先于 click 会让 ✕ 取消也先存上，relatedTarget 判别交给按钮处理）；「从源同步」禁用态原因提示——disabled 按钮自身不触发 tooltip（Chromium 对禁用元素不发指针事件），包 span 挂 title 使「已与源一致/先卸载」悬停可见。）。截图双主题复验。

- **`.dskin` 体积/文件数上限放宽（给创作者的字体与 4K 素材留足空间）**：压缩包 64MB → **256MB**、解压后合计 256MB → **1GB**、文件数 5000 → **10000**——解压/压缩比仍约 4:1，zip 炸弹防护不失效。两侧镜像同步（应用侧打包与 pack-skin 工具同一套常量，对拍脚本盯漂移）；i18n `PackageTooLarge` 文案随值更新（双语）；**打包侧补上压缩产物体积检查**（应用侧 create_package 此前只查解压后合计——不可压缩内容多时可能产出安装侧必拒的包，pack-skin 早有此检查，现两侧同口径）。zip 炸弹测试改按常量动态构造（上限+1MB 零字节）。备份侧上限（5000 条 / 256MB）不变。README 与皮肤开发指南数值同步。

- **布局面板打磨（实机截图评审）**：列表加块级浅底（bg-input——给行项共同的面）+ 加大的行距承担分组；无描边无细线；操作钮恒显（渐进披露被否：移入才显示打断直接操作）；行 hover 改白面 bg-surface（灰底上与 bg-surface-hover 近色不可见——实机反馈）；保存行输入框恢复 bg-input 灰面、边框彻底移除（border:none——透明边框占位与内嵌环之间仍夹一圈线成双重描边），聚焦只显**内嵌 1.5px 粗环**一条线、贴齐灰面边缘。

- **两处快捷键钮样式与管理器其它按钮统一**：`.theme-btn.hotkey-btn` 从坐标牌徽章语言（accent 等宽字）改回 `.action-btn` 同族白面 chip——实机反馈徽章风格与其它按钮不统一；组合值保留 mono 字（技术性内容）。

- **热键录制提示并入描述文案**：「Esc 取消，Backspace 禁用」写入两处的 hint 描述（设置页全局热键 + 编辑器皮肤热键），不再出现在按钮旁——实机反馈位置应属描述区；`hotkeySubHint` 键删除（内容并入 hint 文案）；快捷键钮补 `white-space: nowrap + min-width 120px`——编辑器行内最窄时组合值被挤成多行（实机反馈）。

### 修复

- **布局方案不随备份导入恢复（实机测试反馈——备份语义 = 用户全量数据）**：选择性合并导入此前只并入选中皮肤的 `skin_settings`/`loaded_skins`，布局方案被留在备份里进不来；全量导入路径经「导出 → 解包校验 → 目录替换 → load_config」全链路往返测试证伪无缺陷（新增 `export_import_roundtrip_preserves_layouts` 回归锚点）。修复：选择性导入并入布局方案——`merge_backup_layouts` 按 id 并集、备份版胜（与 loaded_skins 并集同款哲学），引用未导入皮肤的方案保留（应用时 skipped 机制单列提示，不在导入侧剃掉）；并入后托盘「布局方案」子菜单同步重建（config 守卫落地后调用，同 capture_layout 死锁教训）；`merge_backup_layouts_unions_by_id_and_backup_wins` 单测覆盖并集语义（本地独有保留/同 id 备份胜/备份独有并入）。

- **布局面板任何操作都「窗口重新打开闪一下」（实机测试反馈）**：保存/覆盖/点重命名/重命名输入提交/删除等每次操作都走 `rerender()` 整体重建遮罩 DOM，`.confirm-overlay` 的 0.15s 淡入随之重演——视觉上等同面板重开。修复：`rerender` 置 `_noAnim` 标记、重绘加 `no-anim` 类摘遮罩淡入（首开照常动画，CSS 注释记依据）。

- **分组/布局输入框聚焦出现「双重描边」（实机反馈回归）**：上一轮把 `.group-edit-name` 基类的 `outline: none` 作为唯一外描边摘除手段，而全局 `input:focus-visible { outline: 1.5px }` 是 0-1-1 特异性、高于基类的 0-1-0——聚焦时全局外描边与内嵌环并存成双环。修复：补 `.group-edit-name:focus-visible { outline: none }`（0-2-0 压过全局），一处覆盖三处输入（分组/布局保存/布局重命名同族）；教训入注释——自绘内嵌环的输入件必须同时摘 `:focus-visible`，只写基类 `outline: none` 不够。

- **全库审查发现的应修项**：
  - **全局热键静默全死（发布阻断级回归）**：`dispatch_hotkey` 拿事件 `Shortcut::to_string()`（crate Display 产出小写修饰词 + Code 名，如 `shift+control+alt+KeyD`）与 `REGISTERED_COMBO` 比对，而簿记存的是配置原串（`Ctrl+Shift+Alt+D`）——两侧永不命中，全局一键显隐（含出厂默认组合）完全失效且无日志；皮肤专属热键因两侧同走 Display 自洽未受波及。修复：**热键簿记一律以 Display 规范化串为准**（注册/换绑/回滚三个写入点全部改存 `shortcut.to_string()`，短路比较同步规范化——皮肤热键的「与全局冲突」前置检查同根获治）；`docs/关键机制.md` 新增勿回归条目；hotkey.rs 补 parse↔Display 往返单测（归一性 + 反证原串不等）。

  - **选择性导入文件夹撞名可静默销毁未选中皮肤（近阻断级）**：`swap_selected_skins` 回退落位（本地无此 id → 用备份文件夹名）撞上承载**不同 id** 的既有文件夹时，会把它改名让位、拷入备份内容，成功路径删除让位备份、Phase 4 prune 再抹掉其 config 条目——无关皮肤数据全失（审查框不展示文件夹名，用户无从察觉）。修复：任何改名/拷贝前预检——回退落位撞上既有文件夹即整体报错（新键 `ImportFolderConflict` 双语）；单元测试覆盖（占用报错 + 占用者分毫不动 + 无暂存残留）。

  - **逐文件夹暂存崩溃无启动恢复（近阻断）**：`.<folder>.old` / `.staging-*`（选择性导入/从源同步/包安装共用的三段式替换）崩溃在「让位 → 拷入」之间会留下「目标缺失 + .old 唯一副本」——扫描器跳过点目录皮肤消失，启动 prune 再把 config 条目永久抹掉。修复：`package::recover_interrupted_folder_ops`（与导入回滚同款旧副本优先语义：staging 清理、.old 还原——目标在也先删半成品），启动时与 `rollback_interrupted_import` 同点调用（便携与 %APPDATA% 双布局）；单元测试覆盖两种崩溃形态。

  - **连带修正**：选择性导入合并后不重建皮肤热键注册表（并入的 hotkey 要等重启才生效——补 `sync_skin_hotkeys_from_config`）；`duplicate_skin` 拷贝失败不清半成品 + 窗口配置克隆不再带入专属热键（同组合在源/副本间重启后不确定归属）；`set_hot_reload` 落盘失败回滚运行时标志。

  - **B 面收尾毛边**：死 i18n 键对 `settings.backupImportNone` 删除（零选中 toast 已被禁用确认钮取代）；reduced-motion 块混入的一行 `overflow-wrap` 误植删除；`.cfg-step-btn` 补进 reduced-motion 按压关闭列表（镜像漏项）；`.hotkey-btn` 注释「设置页唯一」漂移改写（编辑器皮肤热键钮同用该类）；hotkey.rs 同行两句格式滑手归整。

- **布局方案保存/重命名/删除后整个管理器卡死（std::Mutex 同线程重入死锁）**：`capture_layout` 与 `set_layouts` 持 `state.config` 锁守卫时调用 `tray::rebuild_tray_menu` 同步托盘子菜单——重建链路的 `build_layouts_submenu` 要再取同一把 config 锁，`std::sync::Mutex` 不可重入，命令线程永久阻塞、config 锁此后恒被占用，一切触碰配置的后端 IPC 全部挂起。修复：配置写入收进 `{ }` 块先放守卫再重建托盘菜单（与 `set_language` 同款既有模式）；`docs/关键机制.md` 稳定性约定新增「持锁守卫不得跨会再取同一把锁的调用」勿回归条目。

- **备份导入审查清单：皮肤名跟随管理器语言 + 行内信息收敛**：双语皮肤的名称此前恒显示 `name`——后端 `BackupSkinInfo` 只有 `name_en` 没有 `bilingual` 开关，前端 `dispName()` 无条件回退中文名；后端补 `bilingual` 字段下发，审查清单名称随管理器语言切换（中显中文名、英显 `name_en`）。行内信息收敛为三件：名称 + 版本号 + 权限胶囊——文件夹 id 属实现细节不再进审查视图（`.backup-skin-id` 更名为 `.backup-skin-ver` 并只载版本号）。

### 文档

- **皮肤开发指南新增 §8.4「预览图（preview.png）」**：预览图尺寸事实与建议规格一次讲清——截取产物 = 皮肤窗口当前物理像素尺寸（逻辑宽高 × DPI 缩放，非固定值，先调好窗口再截）；管理器显示区固定约 226×112 CSS px（约 2:1，contain 缩放，任何尺寸都能正确显示，纯展示无文字叠加）；手工设计图建议 904×448（452×224 下限）/ PNG 或 JPG / ≤200KB；文件名 preview.png > preview.jpg > preview.jpeg 取首个命中，随 .dskin 分发（不在打包排除清单）。§1.1 文件清单与 §9 发布检查清单同步加交叉引用。

## [1.2.0] - 2026-08-26

### 新增

- **管理器内打包 .dskin（编辑器「操作」分区新按钮，实机反馈落地）**：已安装皮肤一键打成 `<id>-<version>.dskin` 分发包——点击出保存对话框指定输出位置（默认文件名与 pack-skin 同约定），toast 显示落盘路径。后端新命令 `package_skin`（ManagerOnly，策略表 46/107）+ `package.rs::create_package`（装载校验先行：manifest 合法 + entry 存在才出保存对话框——打出去的包必须能装回来；skip 清单/Deflated/正斜杠路径/256MB 体积上限与 pack-skin 手工镜像，`.tmp` 原子就位；镜像对拍脚本新增 SKIP_FILES/SKIP_DIRS 两项一致性检查——26 项）。i18n 双语键（editor.packageSkin / packagedTo + 后端 PackageCreateFailed）；实机测试清单 §2 补条目（AGENTS #8）。

- **实机测试纪律落地（`docs/实机测试清单.md`，仅开发仓库）**：13 章覆盖安装/皮肤安装/窗口行为/管理器与日志窗/皮肤设置/权限安全/布局备份/语言主题/快捷键/日志/更新/卸载/稳定性全功能面，条目 = 步骤 + 预期、历史事故带 ⭑ 回归标记（本轮实测暴露的「标题栏拖动长期缺失」「首装导出废包」已收录为回归条目）；AGENTS.md 新增硬性约定 #8——新增/变更功能必须向清单补条目，每次正式版发布前维护者本人用发布候选安装包完整过一遍、不过不得发布；`docs/公开发布流程.md` 新增第 0 步「发布前实机测试（不可委托）」。

- **命令策略表 `policy.rs` + 完备性测试（「忘了设防」从人肉审查项变成构建失败）**：全部 106 条 IPC 命令在 `src-tauri/src/policy.rs` 的 `COMMAND_POLICIES` 逐条登记闸门档（八档：ManagerOnly 45 / Perm(权限常量名) 38 / CallerSkin 免权限基线 8 / ControlTarget 7 / SkinLabel 3 / LogWindow 2 / Ungated 2 / AnyPerm 1）——攻击面单点可读，安全审查只读这一个文件即可枚举「谁能调什么」。表不改变运行时行为（闸门仍在各命令函数体）；价值在三个测试：`policies_complete_and_marked`（表 ↔ lib.rs `generate_handler!` 清单双向同集合无重复 + 每条命令函数体出现登记档位的闸门标记——标记含权限常量名、错档即红；标记不带结尾 `?`，因 take_hotkey_error 返回 Option 用 `.ok()?` 形态消费闸门）、`perm_const_names_resolve`（常量名经 `perm_const_value` 锚回真实常量，拼错 panic、常量改名/删除编译失败）、`gate_distribution_snapshot`（各档计数钉住，意外降档被计数变化发现）。立表即校准两处登记认知：take_hotkey_error 的 Option 形态、open_skin_devtools 实为 SkinLabel 档（皮肤 F12 转发通道 + 运行时 dev 开关，release 恒 no-op）而非管理器命令。AGENTS.md 硬性约定 #4（新命令必须登记）与关键机制双版同步。cargo test 126 过。

### 变更

- **公开发布流程改白名单制 + 取消三类内容同步**：公开仓库的 `docs/` 从「默认全量同步减排除项」改为「默认排除 + 白名单四份放行」（关键机制双版 + 皮肤开发指南双版；设计系统/交互动画/已知问题/开发仓库/公开发布流程均为内部资料，新增内部文档默认不再外泄）；新增取消同步：`.agents/` 本地技能库（1.0.4 曾纳入，公开仓库下次同步时删除该目录）、`skills-lock.json`（随技能库）、`tools/win32-probes/`（纯开发调试探针）。放行文档内指向未放行内容的引用统一注明「仅开发仓库留存」（关键机制双版文首总注 + 设计系统引用处）；README 双版项目结构树的 docs/ 列表收窄到同步的四份。同步脚本（白名单 case）与核对清单同步更新；过滤器经 git bash 端到端模拟验证（181/383 文件同步，禁入区零命中）。
- **审查报告归档与文档冗余清扫**：`docs/审查报告-2026-08-24.md` 删除——其高/中/低条目已全部闭环，一次性文档归档于 git 历史（公开发布流程的排除清单与同步脚本同步摘除该文件，清单回到三项）；`docs/已知问题.md` 备查条目改写——管理器/日志窗早已是定制 `apply_native_frame`（旧文所称「tao 原生无边框」与现状不符），且与皮肤 `install_frameless` 是两条刻意不同的配方、勿互套；`docs/开发仓库.md` 远程名与红线改按两克隆实际配置（本克隆 `origin` = 私有 Gitee 开发仓库、`dev-github` = GitHub 备用；Release 克隆 `origin` = 公开 GitHub、main 分支——旧文「本地 origin 指向公开 Gitee 仓库」已不成立，红线改为「先确认身处哪个克隆」）。交互动画/设计系统/双版指南/关键机制巡查无多余内容（已删探针的引用仅剩两处有意指向 git 历史）。
- **审查报告收尾全量修复（工-2/3/4/5/6/7/8/10 + 前端低危余项）**：
  - **工-6/工-7（pack-skin 镜像语义收口）**：`min_host_version` 校验从严格数字段改为与安装端 `update::parse_version` 同口径（数字前缀截断、非数字起始段计 0）——消除「能装却打不出包」（"1.2-beta"/"1.0.x" 等写法此前被误拒），非严格形式只提示不拦；window 默认值钳制补提示式镜像（宽高 [1,10000] / opacity [0.1,1.0] / zoom [0.5,2.0] / refresh_seconds ≤24h——声明值会被钳时打包侧打印「安装生效值 ≠ 声明值」，不改包内容、不拦截）。loader.rs 注释与 AGENTS.md 约定 #2 同步；pack-skin.exe 已重建入库，冒烟打包双路径（正常 + 提示）通过。
  - **工-3（CI 护栏）**：新增 `tools/check-pack-skin-mirror.py`——对拍 types.rs 四个结构体字段与 serde 属性、SkinSettingKind 22 变体保序、默认值函数、安全上限四常量、校验函数 token 流、window 钳制字面量、parse_version 逐字一致，共 24 项，漂移即退出码 1；CI 新增两步（镜像对拍 + pack-skin release 构建），约定 #2 从零护栏变构建失败。
  - **工-8**：卸载补删 `StartupApproved\Run\driftlet`——任务管理器「启动」页不再留没有 Run 值对应的过期记录。
  - **工-10**：vendored `Cargo.lock`（tray-icon / tauri-runtime-wry）撤出跟踪 + gitignore——重新 vendor 时不再有 lock 噪音。
  - **前端余项**：sys-monitor 内联「可用/free」改走 i18n 表（新 `freeSuffix` 键）；`fileUrl` 协议源三处口径统一为 api.js 的平台分支式（Windows = `http://skin.localhost`，余者 `skin://localhost`——driftlet.js 封装与 power-tools 演示同步）；tsconfig `include` 补 examples（driftlet.d.ts 漂移纳入 `npx tsc --noEmit`——mediaSeek 漏封装这类漏网今后可被发现）；types.rs 权限字段注释漂移修正（补 5 个低危权限名 + 免权限基线口径）。
  - 验证：镜像对拍 24 项全过、cargo test 128 过、tsc --noEmit 0 错、vite build 通过、pack-skin 冒烟打包通过。
- **tools/ 探针与工具清理（19 删 / 14 改 / 2 修）**：删除壁纸层与辅助窗时代探针 13 个（wallpaper-probe / wp-black-diag / wp-clip-test / wp-defview-test / wp-dump / wp-dump2 / wp-input-test / wp-plain-child-test / wp-repaint-test / wp-spawn-workerw / wp-xproc-anchor-test / find-defview / shellspy——皮肤永不嵌入壁纸层后这些结构诊断全部无的放矢；关键机制双版壁纸层条目改指 git 历史）、一次性事故产物 4 个（crop_tm_screenshot / find_white_dots / upscale_tray20 硬编码他机路径；wind-verify 硬编码窗口坐标 + 辅助窗时代）、事故专用件 2 个（cmp 需 Rainmeter 对拍；childwatch 查「皮肤是否为 WorkerW 子窗」的死结构）。14 个引用已删示例皮肤「Example Clock」的探针改为经新增 `_common.ps1` 的 `Find-SkinWindow` 按「属主进程 + 标题排除（管理器/日志窗均叫 Driftlet）」定位（C# 层改收 HWND 参数；test-desktop-menu 本就 PID 枚举、标题匹配改排除式）——探针从此不随示例皮肤更替失效。make-tray-icon.py 默认变体 L 依赖的源图不在仓库（工-2）——默认改 L2（参数化 Logo 形态、自包含），L 保留但缺图时报清晰错误；probe_icon_render.py 的 exe 名 Driftlet.exe → driftlet.exe（工-4）。ctprobe.ps1 行尾归位 CRLF（其余 6 个 LF 脚本均在删除列）。PSParser 全量解析 0 错、py_compile 过、Example Clock 残留清零。
- **`open_external` 裁撤本地路径臂（不兼容变更）**：本地绝对路径目标整体拒收——此前靠 35 项可执行扩展名黑名单为 ShellExecute 本地文件设防，是负枚举：清单追不上执行面（新文件类型/系统组件持续增加），且曾被尾点/尾空格绕过（`"RUN.EXE."` 剥尾后真执行）。URI 白名单收窄为 http(s) / mailto / ms-settings 三类（trim + 大小写不敏感；http(s) 仍走 `open_link`/`system` 分层），`is_blocked_executable` 黑名单整体删除。确需打开本地文件的皮肤改走 `shell` 权限的 `run_command`（高危、安装页明示）。perms.js / i18n 双语言 system 描述改「打开网页/邮件/系统设置链接」；toolbox 外链卡改「打开链接」（拒绝演示目标文案同步）、driftlet.js/d.ts 注释同步（顺带补 `mediaSeek` 封装——审查前端 M3）；双版指南（§2.3 表/速查表/§5.3 打开链接小节与电源/文件小节的交叉引用）、关键机制双版、双版 README 同步。cargo test 126 过。
- **权限显示名校准：`file_system`「文件读写」→「任意路径文件读写」、`control`「皮肤控制」→「皮肤窗口控制」**：两个显示名与权限实指有差距——`file_system` 旧名与免声明的沙箱内文件读写同词，用户会系统性低估「整盘任意路径 + 删除」的真实范围；`control` 旧名未体现「含其他皮肤」的跨皮肤范围。i18n 中英双语 4 键改（en: "File access"→"Arbitrary file access"、"Skin control"→"Skin window control"），安装引导页与配置页页眉胶囊经 i18n 自动生效；README 双版权限清单同步（双版开发指南与示例皮肤本就使用准确描述词——§5.3 小节标题「任意路径文件读写」「皮肤窗口配置」、power-tools 文案——零改动）。
- **`media` 与 `media_info` 合并为单一 `media` 低危权限（读取与控制同权）**：三条读取（`get_volume` / `get_media_info` / `get_audio_spectrum` 环回）从 `media_info` 并入 `media`——同一族「当前在放什么」的播控面不再拆分读/控两个权限名；`media_info` 名字退役，旧声明按「未知名一律忽略」处理（该名字仅在本仓库示例中出现过，无第三方迁移面）。media-hub 声明改 `media` + `mic` + `notify`（v1.3.0，页面双语注释同步）；perms.js / i18n 移除 media_info 条目、`media` 描述扩为读取+控制（「媒体与音量 / Media & volume」）；双版指南（§2.3 表 12 权限、速查表、§5.2 结构、§5.3 读取同权注、示例表）、关键机制双版（十二权限条目——media 读取与控制同权 + media_info 退役注）、双版 README（13→12）、driftlet.js/d.ts 注释同步。cargo test 120 过、vite build 通过。
- **`open_external` 分层：新增低危权限 `open_link`（http/https 网页链接专用）**：目标是 `http(s)://` 网页链接时声明 `open_link`（低危）或 `system`（高危）任一即可；`mailto:` / `ms-settings:` 目标仍需 `system`（本地路径目标当时仍需 `system`，同批次随后整体裁撤——见「`open_external` 裁撤本地路径臂」条）——**`system` 自身能力不受影响**（全目标可用，声明过 `system` 的皮肤零迁移）。只开网页的皮肤从此可以全低危：deepseek-balance 摘掉 `system` 改 `open_link` + `notify`（v1.4.0）。实现：分层判定纯函数 `open_external_required_perms`（单测钉住：http(s) 大小写/前后空格、mailto/ms-settings/本地路径/ftp/file 均归 system），闸门 `require_any_perm` 任一放行（拒绝时报首个权限名——低危在前，提示可只声明低危）。perms.js / i18n 双语言收录 open_link（低危蓝）；双版指南（§2.3 表 13 权限、速查表、§5.3 打开链接小节改分层结构、示例表）、关键机制双版（十三权限条目 + open_link 分层机制）、双版 README（12→13——该 13 态与 media 合并同提交（056dbe6）落地，从未单独发布即回落 12）、driftlet.js/d.ts 注释同步。cargo test 120 过。
- **权限模型重构：8 → 12 权限、两档 → 三档分级（含不兼容变更）**：
  - **拆分**：`show_notification` 从 `system` 拆出为新低危权限 `notify`（可见打扰、无数据面，与电源动作不同级）；`system` 余 6 条（open_external/锁屏/关显/睡眠/关机/清回收站）仍为高危。
  - **新增低危档（蓝色徽标）**：`media`（音量/播控，原中危下调）+ 4 个新权限——`notify`、`sys_info`（只读系统信息 13 条：CPU/GPU/内存/磁盘/网络/操作系统/电池/显示器/系统主题/进程/空闲时间/前台窗口——曾按「只读即免权限」不设闸，但含活动监视面，单列低危让安装页可见）、`media_info`（get_volume/get_media_info/get_audio_spectrum 环回——读取与 `media` 控制分家）、`network`（`http_request`；复活未发布即取消的历史名字为新语义——闸「绕 CORS 读响应、任意方法/头」的能力增量，页面 fetch 的受限通道仍在闸外）。
  - **不兼容**：此前免权限的 16 条只读命令与 `http_request` 现在未声明即 reject（报错响亮，形如「皮肤 'x' 未声明权限 'sys_info'」）。迁移：用到只读系统信息的皮肤在 `skin.json` 加 `"sys_info"`（音量/正在播放/环回频谱加 `"media_info"`），用到 `show_notification` 的加 `"notify"`，用到 `http_request` 的加 `"network"`。
  - 免权限基线不变：皮肤目录沙箱文件、自己 schema 的设置读写、日志、广播、control 族「作用于自己」臂。
  - 实现：后端 4 个 PERM_ 常量 + 18 条命令换闸（16 条信息/网络命令签名补 window 过 require_perm——管理器前端不调这些命令，已核实无自伤；无法再直调命令的 probe_hardware_info 探针移除）；前端 perms.js KNOWN 表三档化 + RISK_ORDER 加低危位 + 低危蓝样式（信息色 --accent，与 toast.info 同口径）+ i18n 双语言新档与 4 组名/描述；示例皮肤 sys-monitor（+`sys_info`，「免权限」卖点改「低危只读」，v1.1.0）/ media-hub（+`media_info` +`notify` 并摘掉 `system`，v1.2.0，页面双语注释同步）/ deepseek-balance（+`notify`，v1.3.0）；driftlet.js/d.ts 权限注释全量更新。双版开发指南（§2.3 十二权限表、速查表、§5.2 改低危双权限结构、§5.3 拆分与档位标注、http_request 从 §5.4 迁入 §5.3）、关键机制双版（十二权限条目 + 免权限基线清单 + network 复活新语义）、双版 README（8→12、两档→三档）同步。cargo test 119 过、vite build 通过。

### 变更

- **备份导入审查框视觉与确认框体系对齐（修「与皮肤删除提示割裂」实测反馈）**：皮肤清单从带边框的卡片盒改为与 `confirmDialog` 同源的克制语言——无框文字清单（层级靠字重与间距承担，符合设计系统「不靠发丝描边分层」）、高危警示从填充色块改纯文字行；图标/确认按钮语义色跟内容走——含高危权限才 danger 红，否则 accent 蓝（与删除确认页的图标配色语义一致）。
- **提权处理从「自动降级」改为「只提醒」（全部降级机械移除）**：自动降级两轮实测不可行后定案——检测到 `TokenElevation` 即弹原生消息框说明影响（shell 权限皮肤可静默以管理员权限执行命令；Explorer 拖 .dskin 被 UIPI 拦截可改用双击/文件选择器），「是」= 写 `allow_elevated` 持久放行并继续（以后不再提示），「否」= 退出。移除全部降级机械：任务计划代起（安全软件行为检测 Behavior:Win32/Execution.A!ml 实测命中 + 对真 Administrator 原理上无效——已明令不用）、SAFER 受限令牌与 CreateProcessWithTokenW/AsUserW（依赖 SeImpersonate/SeAssignPrimaryToken 管理员特权——标准用户被 UAC 提权拉起时根本不存在，双臂实测全挂）。`Win32_Security_AppLocker` feature 摘除；elevation.rs 收敛为「检测 + 提醒」（`enable_privilege` 保留给 skin_api power 使用）；i18n 键 DemoteFailed → ElevatedNotice（文案改提醒口径）。关键机制双版条目与 README 双版同步。
- **提权降级从任务计划代起改为 SAFER 受限令牌（Chromium 沙箱 / Run as Limited User 同款 API 链）**：`SaferCreateLevel(NORMALUSER)` → `SaferComputeTokenFromLevel`（当前进程令牌）→ `CreateProcessWithTokenW` 代起普通权限副本——一条路线覆盖 UAC 拉起与真 Administrator（剥自己的令牌，不依赖交互会话令牌）；该族 API 在不同账户形态下脾气不一，受限令牌按三路级联计算（空句柄进程令牌 → 显式句柄 + TOKEN_DUPLICATE → `CreateRestrictedToken(LUA_TOKEN)` 老牌路线——真机 Administrator 实测 `SaferComputeTokenFromLevel` 报 ERROR_GEN_FAILURE(0x8007001F)），代起尝试两臂：`CreateProcessWithTokenW`（SeImpersonatePrivilege）→ `CreateProcessAsUserW`（SeAssignPrimaryTokenPrivilege——策略只剥前者时接住）；特权启用均为尽力、失败不挡路（标准用户被 UAC 提权拉起时令牌里这些管理员特权根本不存在——真机实测「privilege not held by this token」，该形态只能落到明示框）。**任务计划代起永不使用**（维护者明令：其代起行为触发安全软件检测 Behavior:Win32/Execution.A!ml）。**任务计划代起路线移除**：对真 Administrator 原理上无效（交互令牌本身提权，子进程照样提权、再代起撞「任务正在运行」——用户实测 Administrator 机器弹窗无法启动），且程序化建任务代起被安全软件行为检测命中（Behavior:Win32/Execution.A!ml，同机实测）。`CreateProcessWithTokenW` 需要的 SeImpersonatePrivilege 先经 `enable_privilege` 启用（早年该路线 ACCESS_DENIED(5) 即未启用所致）。**降级失败口径修正**：原「弹窗 + 硬退出」（对降无可降环境 = 永久拒绝服务）改为明示一次由用户决定——「是」写 config.json 的 `allow_elevated` 持久放行并继续（以后不再提示），「否」退出；AppConfig 收录该字段（save_config 不丢）；`DRIFTLET_ALLOW_ELEVATED=1` 放行闸不变；debug 默认不降级、`DRIFTLET_FORCE_DEMOTE=1` 可测。1.1.2 时代残留任务与 `--demote-cleanup` 参数由 `cleanup_handoff_task` 自清。关键机制双版提权条目整段重写、README 双版安全模型条目同步。

### 修复

- **`persist_allow_elevated` 在首装机上写极简 JSON 被判损坏重置——标记随 .bak 丢弃、每次启动照弹窗**：首装从未落盘时 config.json 不存在，原实现写 `{"allow_elevated": true}` 极简 JSON，而 `load_config` 要求 `version`/`loaded_skins`/`skin_settings` 三个无默认值必填字段——缺失即判 `Config corrupt, backed up`（用户日志实证），写入的放行标记连同文件被改名丢弃，下次启动照弹。修法：打底改用 `config_base_for_flag` 纯函数——已有配置形状完整（必填三键在）才按原值保留只补标记，否则用完整默认配置打底；新增测试（缺失/极简/形状不全/形状完整四形态打底都必须能被 AppConfig 解析，完整形状原值保留）。受影响机器用新构建再点一次「是」即自愈（写出完整配置 + 标记，此后不再提示）。
- **自启动开关并发/残留态误报 os error 2**：auto-launch 的 `disable()` 在 Run 值不存在时报「系统找不到指定的文件」（注册表 DeleteValue 找不到值）——设置页反复随机点开关的并发同向调用会撞出（用户实测极小概率弹错）。新增统一入口 `sync_autostart`（串行化 + 「已关闭」按目标已达处理），`set_autostart` 命令与备份导入的 `rebuild_runtime` 改走同一入口。
- **`load_config_normalizes_language_field` 测试在中文环境假性通过（英文 CI 报错实证）**：AppConfig 的 `version` / `loaded_skins` / `skin_settings` 是无默认值的必填字段，测试只写 `{"language":...}` 的极简 JSON 会被判损坏重置——中文环境机器上 `default_language()` 兜底成 zh-CN 恰好等于断言值而假过（归一化路径根本没被走到），英文 CI（windows-latest）兜底成 en 立刻露馅。改测完整结构 JSON，真正走归一化路径、任何 locale 下都稳定。
- **管理器/日志窗标题栏拖动恢复 + 备份审查清单鲁棒性**：① 标题栏拖动——tauri 的 drag.js 认 `data-tauri-drag-region` 属性，而重设计（6bae802，07-21）把属性摘了、此后管理器长期无拖动机制（原生窗框无 WS_CAPTION 也不存在系统级拖动）：`.titlebar` 挂回 `data-tauri-drag-region="deep"`（子元素点击都触发拖动、按钮被 drag.js 自动排除），capabilities 补 `core:window:allow-start-dragging`（default.json 主窗 + log.json 日志窗；主窗另补 `allow-internal-toggle-maximize` 供双击最大化）——缺权限时 invoke 被 ACL 静默拒绝是「拖不动」的直接成因。gen/schemas 随构建 regenerate 一并提交（AGENTS.md 约定 #7）。② `inspect_backup` 改直读 manifest 本体（不走 loader 的 entry 存在性装载校验）——入口文件缺失/损坏的皮肤不再从审查清单静默消失；备份测试夹具补 index.html 更贴近真实 + 新增「导出→审查」往返测试。③ 首装即导出即导入产废包——首次安装从未改设置时磁盘上没有 `config.json`（启动只建目录、首次改设置才落盘），导出的 zip 不含 `config/config.json`，导入侧 `validate_backup` 必报「不是有效的 Driftlet 备份」（1.1.0 起即存在，用户首装→不装皮肤→备份→导入实测命中）：`export_backup` 缺该文件时把默认配置补进包（该场景内存配置与默认等价——所有设置即改即存，文件缺失 = 从未变更）；新增首装路径导出测试。cargo test 135 过、vite build 通过。另：从零全量重建（cargo clean + 全量编译）验证通过；构建期那条「正在创建库 driftlet_lib.dll.lib」是 MSVC 链接器的信息性输出被 Rust 当 linker_messages 警告显示（cdylib+staticlib 混合 crate 类型的既有现象），无害。
- **备份导入补权限审查（第二轮审查 A-M2 待决策项落地）**：布局备份导入从「危险确认框即执行」改两段式——`inspect_backup`（选包 → 解包校验 → 返回包内皮肤清单与权限声明 + 备份时宿主版本）→ 确认框展示皮肤清单 + 权限胶囊（perms.js 同口径，含高危声明时顶部警示行）→ 确认后 `import_config(path)` 执行。备份导入不再绕过 .dskin 安装引导页的权限展示（此前第三方分享的备份可静默落地高权限皮肤）。新命令 `inspect_backup` 已登记策略表（ManagerOnly 45/106，完备性测试如期拦住漏登记）；i18n 双语新键（backupReviewEmpty/backupReviewHighWarn）+ style.css 审查清单样式；关键机制双版导入条目与 README 双版备份行同步。cargo test 133 过、vite build / tsc 通过。
- **第二轮独立全库审查（四路并行子代理 + 主控逐条亲验，误报当场判撤）的修复**：
  - **A-H1（备份导入崩溃回滚被启动顺序架空——数据静默丢失）**：`resolve_portable_dir` 的可写性探测会先建目录，把「目录缺失」的崩溃现场抹成「两者都在」，回滚分支沦为死代码，唯一完好的 `.import-old` 被当残留删除。修复：回滚提前到目录创建之前（便携与 %APPDATA% 回退两种候选位置各跑一次）+ 语义改「`.import-old` 存在即回滚」（不再以「两者都在」推断成功——②中途崩正是两者都在）。backup.rs 新增三形态回滚测试。
  - **B-F1（config.json 的 `language` 字段是脚本注入向量）**：`load_config` 对它零校验、值直通皮肤桥 `<script>` 烘焙，而 serde_json 不转义 `/`——`"</script><script>…"` 破标签即注入（备份导入是投递向量）。双闸：`load_config` 经 `i18n::normalize` 归一化 + 桥烘焙串（language/theme/hostVersion）统一 `</` 转义（与 baked_settings_json 同款）。config.rs / protocol.rs 各新增测试。
  - **D-A（重定向跨主机转发敏感头——相对 ureq 自动跟随的回退）**：手动跟跳 loop 每跳原样重发全部自定义头，而 ureq 自动跟随默认剥 content-length/cookie/authorization（RedirectAuthHeaders::Never）。新增纯函数 `headers_for_hop`：跨源（scheme/host/port 任一变化）剥授权类三头、转 GET 丢体时另剥 content-length/content-type。
  - **D-B（IPv4-mapped IPv6 穿透 SSRF 防线）**：`http://[::ffff:127.0.0.1]/` 经双栈实际打到 127.0.0.1——v6 臂改先查自身 loopback/unspecified、再 `to_ipv4()` 折回 v4 同规则（注意：`to_ipv4` 会把 `::1` 折成 0.0.0.1，顺序写反会把 loopback 漏拦——初版即犯、被既有测试当场抓住）；v4 段检查补 `0.0.0.0/8` 本网络段。
  - **A-M1（reset_skin_config 锁不全）**：曾只持 lifecycle_lock——与 install 族（安装/卸载/导入/reload_all 持 install_lock）无互斥，且删 settings.json 不持 settings_lock（交错时「重置后设置复活」）。改持 lifecycle+install 双锁（`lifecycle_guards`）+ 删除段持 settings_lock，锁序与文档一致。
  - **C-F1（设置页内 toast 被遮罩压住不可见）**：`.toast` z-index 300 < 弹层 400——设置面板开着时全部操作反馈不可见，提到 500。
  - **D-C/D-F（策略表测试的结构性弱点）**：`fn_segment` 段尾从「下一条命令」收紧为「最近的下一个顶层项」（末条命令的段曾混入后续 helper 与整个 tests 模块——其中恰好出现的同档标记文本会造成误判放行）+ 标记核对前剥行注释。
  - **C-F2/F3/F4 + D-D（镜像对拍脚本解析洞）**：check-pack-skin-mirror.py 的 `block()` 花括号配对改字面量/注释感知（`let c = '}';` 曾截短块）、字段正则不再被含逗号类型（`HashMap<String, String>`）静默丢弃 + 属性错挂修复、枚举变体剥行尾注释、MIN_ZOOM/MAX_ZOOM 抓取限同行、rename_all 判定限枚举属性区、validate_skin_id 补长度上限对拍。24 项对拍全过。
  - **低危批**：snap.rs 超宽窗哨兵 `i32::MIN` 溢出（release 环绕把窗口吸到 -2³¹ / debug panic）改空切片 + 测试；audio.rs 采集线程 COM init 失败路径的未配对 CoUninitialize 改条件释放；fs.rs 写路径 base64 先按展开上界拦超长输入再解码；factory.rs 建窗宽高补钳 1–10000（持久化配置手改侧曾直通）+ DRAG_SAVE_STATE 卸载回收；d.ts 补 `DriftletMediaInfo.seekable`；skin-editor 步进器纳入原地同步；探针窗口定位补「Tauri Window」类过滤（IME 辅助窗误锁）；open_skin_devtools 注释口径改如实（开关不经 cfg 门、release 可达）。
  - **误报澄清**：第一轮审查（另一 AI）H1 配套所称「inet_aton 数字字面量绕过」经 url crate 源码实证（host.rs 的 WHATWG 宿主解析预归一化）在现行解析器下本就拦得住——`parse_inet_aton` 是纵深而非主力防线，文档口径已改如实。
  - **留待决策**：备份导入的权限审查缺口（备份包内皮肤的权限声明未在导入时展示——设计决策）；**已登记不修的残余面**见 `docs/已知问题.md` 新「复核记录的残余面」节（DPI 快照不刷新 / __fs__ 每请求重扫 / skin:// 指纹面 / app_log 合批无上限 / 探针自写抑制形态）。
  - 验证：cargo test 133 过（+5）、vite build / tsc / 镜像对拍 24 项 / ps1 解析全绿。
- **审查报告第二批次修复（剩余条目经逐条独立复核——M1 与 工-1 判定为误报，不改）**：
  - **M2（沙箱读 TOCTOU 无界分配）**：`skin_read_file` 与任意路径版统一走新抽取的 `fs::read_capped`（`take(max+1)` 流式 + 按实际字节再审）——metadata 预检与读取之间文件被换大不再做无界分配；`CapReadError` 区分 IO 错误与超限，两侧报错文案口径不变。新增上限测试（恰好超限即拒）。
  - **M3（右键菜单生命周期无锁）**：皮肤右键菜单的「刷新/卸载」从直调无锁 impl 改为 spawn 闭包内先取 `lifecycle_guards`（lifecycle → install 双锁）——与管理器命令/热重载同一条串行化路径，消除与并发加载/热重载的竞态；`lifecycle_guards` 与 `load_skin_impl` 的注释口径同步（菜单路径现已在外层串行之列）。
  - **media-hub 轮询隐藏暂停（前端 M5）**：媒体信息 1 秒轮询补 `document.hidden` 检查（与 specTick 同口径）+ 恢复可见即补查一次——此前页面隐藏期间轮询空转（无谓 CPU 与 SMTC 查询）。
  - **media-hub 封面 mime 前端白名单（前端 H，纵深）**：`data:` URI 的 mime 落值前过五格式白名单（非预期回退 jpeg）——后端 `sniff_image_mime` 的 magic-bytes 白名单本就封死，此为双保险，非直接可利用面。
  - **L1**：`window/registry.rs` 的 `all_hwnds` 死代码删除（全库零调用；其注释设想的用法恰是「HWND 快照会被系统回收复用」的反模式）。
  - **L4**：`media_seek` 入参钳制（非有限值归 0、范围钳 0–86400 秒）——负值/巨值不再直达 SMTC。
  - **L5**：`slugify_skin_id` 滑出保留设备名（如 "con"）时落哈希形态兜底（此前下游 `validate_skin_id` 才拒、安装才失败）；新增单测（保留名全落合法 id、正常名字照旧 slug）。
  - **面板漂移**：配置面板宽高输入 min/max 从 50–4000 对齐后端钳制 1–10000（`MAX_DIMENSION`）。
  - **文档口径清扫**：关键机制双版长权限条目三处自相矛盾残留改齐（SSRF 段落旧「重定向残面接受」句 vs H1 新条目、file_system「两根」vs H2 四根、UNC 交叉引用已裁撤路径面的 open_external）+ 锁条目补菜单路径；指南双版 media_seek 钳制注、media-hub/toolbox 示例行同步。
  - cargo test 128 过、vite build 通过。
- **`http_request` 的 SSRF 防线被 302 重定向整体绕过（审查 H1）**：ureq 自动跟随的 1–3 跳不经过 `is_private_host`——公网站点 302 → `169.254.169.254`（云元数据）/ `127.0.0.1`（本机服务）即可把响应体交给皮肤，与 `network` 低危权限「会联网」的声明承诺不符（实质能力增量）。修复：`redirects(0)` 关自动跟随，手动逐跳解析 `Location` 并经纯函数 `resolve_redirect_url` 复检（scheme 限 http/https + 私网判定，上限 3 跳；301/302/303 按浏览器语义转 GET 丢请求体，307/308 原方法原体重发）；`is_private_host` 同步补上 inet_aton 数字字面量形态（`2130706433` / `0x7f000001` / `127.1` / `0177.0.0.1`——`IpAddr::parse` 不认但 OS 解析器会当成 IP，此前落进域名放行分支）——`parse_inet_aton` 纯函数还原后按同一套段检查，溢出段（999.1.1.1）连 WHATWG URL 解析都过不了、按解析失败即拒绝。两个纯函数均有单测钉住（相对 join、内网跳、字面量跳板、file:/javascript: 跳、公网字面量放行）。
- **`file_system` 皮肤可改写更新目录安装包，「立即安装」即任意代码执行（审查 H2）**：便携布局下 `update/` 坐在 exe 旁却不在 `ensure_mutable_any_path` 禁写根内——皮肤整体替换 `Driftlet-update-setup.exe` 并写好版本标记后，用户对「安装官方更新」这个可信动作的确认即执行任意代码。两道修复：① 禁写根从 2 个扩到 4 个（skins/config + `update/` + exe 所在目录——后者覆盖 Driftlet.exe 本体 / WebView2Loader.dll / 卸载器的改写面；便携布局下是其余三根之父、冗余但显式，非便携回退布局四根不相交各自生效），读/列不受限照旧；② `download_installer` 边下边算 SHA-256 写入版本标记（`downloaded-version.json` 增 `sha256` 字段），`install_update` 执行前过新函数 `verified_installer` 三重核对——标记存在且完整（旧版下载的无哈希标记 fail closed，重新下载自愈）/ 标记版本新于当前运行版本 / 安装包文件哈希与标记一致（大小写容忍），缺一即拒、报错统一引导重新下载。哈希一票否决是与改写途径无关的纵深。update.rs 核对全分支单测 + mod.rs 更新目录禁写测试（含不存在目录的重拼形态与前缀陷阱）。cargo test 126 过。
- **perms.js 头注释失同步修复 + 双版指南 skin_broadcast 信任提醒**：perms.js 头部注释仍列已退役的 `media_info`、漏新增的 `open_link`（KNOWN 表本身是对的，注释是 media 合并与 open_link 分层两轮重构的漏网之鱼）——注释与 KNOWN 对齐；双版开发指南 §5.4 `skin_broadcast` 小节各加一句提醒：**免权限 = 无鉴权，channel 名对所有皮肤公开、任何皮肤可向任意 channel 发消息**——接收侧须像对待任意事件源一样校验 `from` 与 `payload`，勿将广播内容当可信输入。
- **`file_system` 权限新增应用数据根变更保护（堵自我提权漏洞）**：`skin_write_any_file` / `skin_create_any_dir` / `skin_delete_any_path` 三条变更命令的目标落在 Driftlet 自身数据目录（`skins_dir` / `config_dir`，便携布局下在 exe 旁）内时一律拒绝。此前声明了 `file_system` 的皮肤可改写**自己的 skin.json** 往 `permissions` 里加项——`require_perm` 每次调用实时重扫 manifest，新权限（如 `shell`）立即生效，等于绕过安装引导页的权限承诺静默提权（改 `config.json`、删其他皮肤同门）。实现：新增 `ensure_mutable_any_path`（先拒 `..` 分量——Windows 沿符号链接逐分量解析 `..`、非纯词法；再 `resolve_location`：全路径存在则 canonicalize 解析符号链接/8.3 短名/真实大小写，否则最深现存祖先 canonicalize + 不存在尾段词法重拼（尾段分量不存在、不可能是符号链接）；前缀比较分量级 ASCII 忽略大小写，兜底缺失数据根与尾段大小写变体——缺失根可被预植 `skins/<id>/skin.json` 绕过安装页确认）。读/列不受限：「整盘可读」是安装页声明过的语义。新增 3 个测试（根内全形态拦截 / 根外放行与前缀陷阱 / 大小写变体与缺失根）；`..` 用例须从字符串构造路径——`PathBuf::join("..")` 在 verbatim 基底（canonicalize 产物）上会词法消掉 `..`（实测陷阱，代码注释已钉住）。双版开发指南（§2.3 权限表 + §5.3 文件小节）、关键机制双版（八权限条目）同步。cargo test 119 过。

## [1.1.2] - 2026-08-23

### 新增

- **提权启动自动降级（`elevation.rs`，任务计划交互令牌方案）**：程序不需要管理员权限（清单 asInvoker，全部能力标准用户可用），但被已提权的终端/启动器拉起时子进程会继承高完整性——皮肤上下文随之提权、`run_command` 子进程全部提权、Explorer 拖 .dskin 进管理器被 UIPI 拦截。现启动最前（Builder/single-instance 之前，父进程无占用无 handoff 竞态）检测 `TokenElevation`，提权则注册一次性任务（`/it` 交互令牌、不传 /ru 免密码）并 `schtasks /run` 立即触发——任务计划服务从交互会话令牌创建子进程，**天然中完整性**（本地探针实证 S-1-16-8192 Medium IL），子进程启动时按 `--demote-cleanup` 参数自清任务。**三条堵死路线成文勿改回**（均实测）：CreateProcessAsUserW（SeAssignPrimaryToken 不发给管理员）、CreateProcessWithTokenW（ACCESS_DENIED 5）、explorer COM 链（GetItemObject 拿不到 IShellFolderViewDual）；`runas /trustlevel:0x40000` 密码提示走 WriteConsole、无控制台即死。降级失败 = 原生消息框提示 + 硬退出（提权不可用是明确需求，不再「记警告继续跑」）；`DRIFTLET_ALLOW_ELEVATED=1` 放行；debug 构建默认不降级（防 `npm run tauri dev` 从提权终端起时断 dev loop），`DRIFTLET_FORCE_DEMOTE=1` 可测。参数引号规则有单测钉住。关键机制双版、双版 README 安全模型同步。cargo test 116 过。
- **`system` 权限扩容五条常用命令 + `open_external` 放行 `ms-settings:`**：新增 `lock_workstation`（锁屏，LockWorkStation）、`monitor_off`（关显示器，PostMessage 广播 SC_MONITORPOWER——不走 SendMessage，广播遇挂死窗口会被同步拖住）、`sleep`（SetSuspendState，自动启用 SE_SHUTDOWN_NAME 且校验 GetLastError ≠ ERROR_NOT_ALL_ASSIGNED）、`power_control`（关机/重启/注销，ExitWindowsEx **不带 force**——未保存数据的应用可阻止，用户看系统级阻止界面）、`empty_recycle_bin`（SHEmptyRecycleBinW 资源管理器同款确认框+进度+音效，先查空、已空直接成功不弹框）。**`system` 刻意没有「启动 exe」的通道**：五条命令全部无路径/无目标参数，open_external 的可执行黑名单原样保留——运行程序仍只属于 `shell` 权限的 `run_command`。open_external 白名单新增 `ms-settings:`（系统设置页 URI，设置应用处理，无代码执行面）。Win32 调用统一 spawn_blocking（不占主线程/async worker）。perms.js 的 system 描述双语言更新（旧文案「打开外部链接或程序」本就不实——程序一直打不开）；toolbox 新增「电源与回收站」演示卡（关机/重启/注销按钮两击确认）+ 外链卡补 ms-settings 演示钮（v1.1.0）；driftlet.js/d.ts 收录五条封装（顺带修正 setVolume/setMute/mediaControl 注释残留 system 应为 media）。双版指南（§2.3 权限表 + 速查表 + §5.3 新小节与 open_external 白名单）、关键机制双版（电源五条条目 + 白名单）、双版 README（系统控制括注媒体拆分后残留音量/媒体一并修正）同步。cargo test 113 过。
- **媒体控制拆分为独立权限 `media`（中危，权限 7 → 8）**：`set_volume` / `set_mute` / `media_control` / `media_seek` 从 `system` 挪出单列——「当前在放什么」的播控面（音量 + 播放 + 进度）与开外链/通知不同级。`system` 保留：打开外部链接/文件、系统通知。media-hub 声明 `system` + `media` + `mic`。perms.js/i18n 收录 media（中危黄）。双版指南（§2.3 八权限 + 速查表 + 小节标题）、双版 README、关键机制双版（八权限 + 分级清单）、CHANGELOG 未发布节同步。
- **媒体进度条可拖动 + 会话择优**：`media_seek` 命令（`media` 权限）按绝对秒寻址（SMTC `TryChangePlaybackPositionAsync`），`get_media_info` 新增 `seekable` 字段（源是否支持寻址——媒体中心式播控常关寻址，false 时进度条锁只读）；**会话选取从「GetCurrentSession（Windows 认为的最近交互）」改「枚举 + 择优」**（正在播放 > 有进度 > 有元数据）——多会话场景（浏览器 + 网易云并存）取到的是正主，此前网易云进度拿不到即因此。media-hub（v1.1.0）进度条可寻址时可拖（透明 range 覆盖承接手势，拖动中冻结轮询回写防抖，松手才寻址）。双版开发指南（get_media_info 小节 + media_seek + 速查表）、关键机制双版（会话择优 + 寻址条目）同步。

### 修复

- **media-hub 进度条寻址后「往左缩再回弹」**：松手 change 后立刻轮询读到的是 SMTC 追上来之前的旧位置，回写把进度条缩回去、下一秒再弹回新位置——寻址后开 1.2s 抑制窗（lastSeekAt），期间轮询不回写进度条；拖动中 transition 照旧即时跟随。
- **media-hub 寻址命中区加宽 + 无进度降级提示**：进度条命中区从 6px 细条加宽到 22px 整条（此前基本点不到）；源不上报时间线（如网易云）时进度区显「本播放器不上报进度」提示（noTimeline，中英双语）而不是留白。
- **toolbox 电源演示卡武装态可被语言切换绕过**：管理器切语言触发 applyI18n 重绘，按钮文案被重置回「关机」但内部仍是武装态，再点即无确认执行——armed 状态登记入 powerDisarmers，重绘前先统一解除武装。

## [1.1.1] - 2026-08-23

### 新增

- **web-view 新增「自动刷新」开关**（boolean 设置项，默认开；v1.0.2）：关闭后定时器不起、恢复可见不补刷——只在手动点刷新按钮时重载；开启后恢复 5 分钟定时 + 隐藏暂停/恢复补刷的既有语义。双版指南 §10 行同步。
- **更新检测自动下载安装包**：发现新版本后后台自动下载 NSIS 安装包（**固定文件名 `update/Driftlet-update-setup.exe`**，下次下载覆盖旧包、安装包不随版本堆积；先写 `.tmp` 再 rename 就位，中断只留一个 .tmp 下次覆盖、失败清掉），**下载完成才弹「立即安装」弹窗**（下载失败/无安装包资产时降级为「前往下载」旧流程）；「立即安装」启动安装包并整站退出（NSIS 等本进程退掉才能覆盖 exe）；**垃圾不堆积三道闸**：固定文件名覆盖 + .tmp 起手清/失败清 + 启动清理（当前版本 ≥ 下载标记版本 = 装上了 → 删安装包与标记）；下载链接钉死公开仓库 release 下载域前缀校验（只接受 GitHub API assets 返回的直链）。设置页开关文案改述「检测并自动下载」。

### 修复

- **皮肤库滚动顶缘渐变淡出**：滚动皮肤库时，顶部「皮肤库」标题栏下的卡片不再被生硬截断——列表滚出顶部后在顶缘 26px 做渐隐遮罩（mask-image，仅在滚动了才启用，静止在顶不遮）。
- **浅色主题下最小化/最大化按钮悬停背景加深**：原先 `--bg-surface-hover` 是 #f0f6fb（近白、在冷纸底标题栏上几乎看不见）——新增 `--win-btn-hover` 令牌（浅色 #dce8f2 冷蓝灰、深色沿用 --bg-surface-hover 的值），悬停反馈在浅色下有明确背景。关闭按钮悬停与深色主题均不受影响。
- **管理器窗控按钮样式回退 + 图标统一重绘**：悬停样式回退到原先的透明底方案（软芯片悬停被否）；三个图标按管理器其它图标的语言重绘——统一线宽 1.5 + 圆角端点/圆角矩形（最小化原为 1px 细线太细、最大化原为直角方框、还原与关闭补上同款圆角端点），最大化↔还原的切换图标与日志窗的同款按钮一并换新。
- **文档残留清理**：双版开发指南的 iframe 内嵌路线示例引用指向已改名的 `examples/kimi-quota`（应为 `web-view`）；web-view 的 style.css 头注释残留旧皮肤名。
- **管理器图片可被拖出窗口**：预览图/品牌 logo 的默认图片拖拽会把图片拖出到桌面——style.css 全局 `img{-webkit-user-drag:none}` 钉死（只拦「元素拖出」，.dskin 拖入安装流不受影响；皮肤侧不动——皮肤可能有合法的页内 HTML5 拖拽用法）。

## [1.1.0] - 2026-08-18

### 新增

- **网页皮肤双路线 + 示例皮肤 `web-view`（iframe 内嵌通用网页皮肤）**——①`entry` 支持 http(s) URL（纯 URL 皮肤）：窗口直接加载站点页面，cookie 持久化；页面无注入桥（无拖动区/右键菜单/命令通道，远程源命令调用由 tauri remote-origin 守卫兜底拒绝）；初始不透明度经 initialization_script 落；`window.refresh_seconds` 定时重载；loader/package 的 entry 校验对 URL 豁免、pack-skin 镜像同步重建 exe。②**iframe 内嵌路线（需要拖动/右键菜单/桥能力时的正解）**：本地外壳页面（桥全功能）内嵌目标站 iframe——cookie 与 WebView2 用户数据目录同罐（登录一次持久化），跨域 iframe 重赋值同 src 即重载；前提是目标站允许被嵌（无 X-Frame-Options / frame-ancestors 限制——实测 kimi.com 不拦）。`web-view`（零权限）：站点地址为设置项（管理器改完即切换）、本地外壳 = 标题条拖动区 + 刷新按钮 + 状态点（加载中闪/完成绿/失败红）、未配置地址时引导态、定时刷新（默认 5 分钟，1–60 可调）+ 隐藏暂停/恢复补刷；iframe 的 `[hidden]` 与 display 冲突已钉死（`#frame[hidden]{display:none}`）。无头 Edge 截图目检通过（引导态居中、外壳 + kimi 登录页真实渲染）。双版开发指南（§2.1/§2.2 双路线说明 + §10 行）、双版 README、CHANGELOG 未发布节同步。
- **皮肤设置页新增文件/文件夹选择器控件（控件 20 → 22 种）**：`skin.json` 声明 `"type": "file"` / `"type": "directory"` 即得路径选择控件（只读路径框 + 「浏览…」+ 清除钮）；**系统对话框由管理器弹**（皮肤窗口拿不到也不该弹系统对话框）——控件按钮调新命令 `pick_path`（require_manager、spawn_blocking 阻塞对话框、tauri-plugin-dialog 既有栈；`file` 控件可用 `filters` 字段限扩展名，后端白名单化为小写字母数字；取消返回 null 不动控件）；值 = 绝对路径字符串（≤1024 字符，空串 = 未选），写回走 `set_skin_custom_setting` 同一管道（皮肤照常收 `desk-window-config-changed` 之外的 `desk-setting-changed`）。与 `file_system` 权限的 `__fs__` 端点组合即解锁「自定义背景图/数据目录」类皮肤。同步面：`types.rs` SkinSettingKind 两个新枚举 + `filters` 字段、`loader.rs` 类型匹配与兜底两臂、`commands.rs` 校验、`skin-editor.js` 渲染与绑定、i18n 双语言、style.css `.cfg-pick` 样式、**pack-skin 手工镜像同步并重建 exe**、controls-demo 新增「路径」组两控件（v1.0.1，值区直接显示路径）、双版开发指南（§4.1 通用字段 + §4.2 表 22 种 + 检查单 + §10）、双版 README 控件数、关键机制双版（新增控件四处同步 + pick_path 链路条目）、CHANGELOG 未发布节。cargo test 111 过、vite build 通过、pack-skin 校验通过。
- **窗口配置变更事件 `desk-window-config-changed`**：皮肤的窗口配置被修改时收到事件（detail = `{ key, value }`，value 为应用后的有效值）——在全部 `set_skin_*_impl` 单点派发，管理器面板路径与 control 皮肤路径口径天然一致（当初抽取 impl 的红利）；key 覆盖 opacity/placement/click_through/position_locked/resizable/zoom/edge_snap/snap_gap/position/size；拖拽/边框缩放引起的变化不派发（皮肤自己拖的 + 防抖通道不宜洪泛）；目标未加载时不派发（窗口不存在）。`driftlet.js` 新增 `onWindowConfigChanged(fn)` 助手。cargo test 111 过、vite build 通过。双版开发指南（控制小节）、关键机制双版（单点派发条目）、CHANGELOG 未发布节同步。
- **`control` 权限降为中危（黄标）**：皮肤控制（作用于他人的窗口配置/生命周期/显隐/清单枚举）从高危降为**中危**——`perms.js` 分级表改 medium，安装引导页与页眉胶囊随之黄标；语义与闸门不变（`require_perm` 二元校验不分档）。双版指南 §2.3 表与分级句、速查表、§5.3/§5.4 小节标题、双版 README、关键机制双版分级清单、power-tools 演示（描述与卡片徽标改黄）、driftlet.d.ts 注释同步。
- **外部文件 URL 直引（`__fs__` 协议端点，`file_system` 权限门）**：声明了 `file_system` 的皮肤可用 `http://skin.localhost/__fs__?path=<percent 编码绝对路径>` 在页面里直接引用皮肤目录外的文件（`<img src>` / CSS `url()` / `<video>`）——不经 JS 内存，浏览器流式加载（base64 命令通道留给数据处理）。身份取自 `UriSchemeContext::webview_label()` 可信 IPC 通道（Referer 可伪造绝不用）；皮肤 id 字符集永不与 `__fs__` 撞名；目标须为存在的普通文件绝对路径；未声明权限一律 404。`parse_fs_query` 纯函数已用测试钉住（绝对路径/存在性/目录拒绝）。power-tools 演示皮肤文件卡新增「预览为图片」按钮。driftlet.js 新增 `fileUrl(path)` 助手。cargo test 111 过（新增端点解析测试）。双版开发指南（§5.3 文件小节）、关键机制双版（协议端点条目）、CHANGELOG 未发布节同步。
- **`file_system` 权限补齐目录能力三条**：`skin_list_any_dir`（列任意目录，与沙箱版 `skin_list_dir` 同款 `{name, is_dir, size}` 结构、目录项排前按名称排序）、`skin_create_any_dir`（建任意目录含多级，已存在视为成功）、`skin_delete_any_path`（删任意路径——文件直删，目录默认只删空目录，整棵目录树须显式 `recursive: true` 防误删）。至此 `file_system` 全家福：读/写/列/建/删。power-tools 演示皮肤文件卡新增「列目录」按钮。cargo test 110 过、vite build 通过。双版开发指南（§2.3 权限表、速查表、§5.3 文件小节）、driftlet.js/d.ts、关键机制双版、CHANGELOG 未发布节同步。
- **`http_request` 降为免权限（network 权限取消，未发布即移除）**：页面本就有 `fetch` 通道（`no-cors` POST 已能外发数据），单独设闸挡不住有心者、只做展示噪音——`http_request` 改为仅经 `caller_skin` 身份校验即可调用（任意 http(s)、自定义头、binary base64 通道、响应头回传、4xx/5xx 照返、4MB 截断等契约不变）；`network` 权限名从 perms.js 分级表与 i18n 移除（未发布，无皮肤声明过）。双版开发指南（§2.3 权限表七项、速查表、小节迁 §5.4 免权限区）、双版 README（7 种）、关键机制双版（七权限 + http_request 免权限条目与理由）、CHANGELOG 未发布节同步。
- **皮肤控制接口自身操作全面免权限**：`skin_set_window_config` / `skin_hide` / `skin_show` / `skin_load` / `skin_unload` / `skin_reload` / `skin_get_window_config` 作用于自己（省略 skinId / 空串 / 传自己 id）时**一律免权限**——自己的窗口自己调（此前仅 opacity 单键免，现全键放开；生命周期对自己也免，fire-and-forget 语义不变）；指定其他皮肤仍一律 `control` 中危门；`skin_list_skins` 例外恒走 control（枚举本质是看他人）。双版指南、关键机制双版、driftlet.js/d.ts 同步。
- **皮肤 API 面归一化（审计后收口，全部未发布、零兼容成本）**——①命名统一入 `skin_*` 族：显隐 `hide_skin`/`show_skin` → `skin_hide`/`skin_show`、任意路径文件 `read_any_file`/`write_any_file` → `skin_read_any_file`/`skin_write_any_file`（与 `skin_load`/`skin_get_window_config` 等同族；危险的任意路径版不再比沙箱版 `skin_read_file` 少前缀）。②**skinId 省略/空串/传自己 id = 作用于自己**（`resolve_control_target` 一处收敛）：桥不烘焙皮肤自己的 id，自操作无需硬编码——`skin_get_window_config`/`skin_set_window_config`/`skin_hide`/`skin_show` 统一该约定；读取自己免权限、改自己仅 `opacity` 键免权限（调暗自己属无害路径，与显隐自己免权限同例），其余一律 control 门。③补 `skin_list_skins`（control）：枚举全部已安装皮肤（id/name/name_en/version/author/loaded/hidden）——此前 control 用户跨皮肤操作只能靠猜 id。④`http_request` 补二进制通道与响应头：`binary: true` 时请求体 base64 解码发送、响应体 base64 返回（图片/字体不再被 UTF-8 替换字符毁损），返回结构新增 `headers`（同名多头保留首值）。power-tools 演示皮肤同步改名与「留空 = 本皮肤」。cargo test 110 过、vite build 通过。双版开发指南（§2.3 权限表、§5 速查表、§5.3 各小节、§5.4 显隐小节）、driftlet.js/d.ts、关键机制双版（control 条目：清单命令 + 省略即自己 + 免权限键白名单）、CHANGELOG 未发布节同步。
- **皮肤显隐接口 `skin_hide` / `skin_show` 与系统主题检测 `get_system_theme`**——①显隐接口：`skin_hide(skinId?)` / `skin_show(skinId?)`，省略 id（或传自己 id）作用于自己**免权限**（仅自身可见性的无害路径，通知式皮肤的「看完即消失/定时再现」），指定其他皮肤并入 **`control` 中危权限**（与窗口配置/生命周期同层）；`skin_show` 只显示**不抢焦点**；变化统一走 `sync_tray_toggle_item` 漏斗（托盘勾选与「已隐藏」徽标联动），用户侧唤回通道（全局热键/托盘勾选）始终兜底。②`get_system_theme`：检测 Windows 系统级浅/深主题（注册表 `AppsUseLightTheme`，返回 `"light"` / `"dark"`，只读免权限、与 §5.2 系统信息同组）——与桥成员 `theme`（管理器主题，可能不跟随系统）区分开，皮肤可按需跟随 Windows 设置。cargo test 110 过、vite build 通过。双版开发指南（速查表、§5.2 新小节、§5.4 显隐小节、§2.3 权限表 control 行）、关键机制双版（无害命令回三枚、control 条目补显隐）、CHANGELOG 未发布节同步。
- **皮肤 API 扩展：主题跟随、自隐藏、HTTP 请求、生命周期控制、皮肤间广播**——①桥新增 `theme` 成员（`__DESK_PP__.theme`，`auto` 由后端 `current_theme` 按本地小时折算成 light/dark，与前端 timeBasedTheme 同规则），`set_theme` 运行时向皮肤窗 eval 更新并派发 `desk-theme-changed` 事件（与语言同款零竞态烘焙 + 推送模式），皮肤可跟随管理器昼夜配色。②`skin_hide`（作用于自己时免权限）：皮肤主动隐藏自己的窗口（通知式皮肤的「看完即消失」；身份取自窗口 label，托盘勾选与管理器「已隐藏」徽标经 sync_tray_toggle_item 漏斗照常联动；唤回 = 全局热键/托盘）。③新权限 `network`（高危）：`http_request` 通用 HTTP 请求（仅 http/https，自定义头与文本负载，超时钳制 1–60s；阻塞 ureq 放 spawn_blocking；**HTTP 4xx/5xx 照返状态码与响应体**、仅网络层失败 reject；响应体截断 4MB 并标 truncated）——突破页面 fetch 的 CORS 限制。④`control` 权限扩展生命周期三条：`skin_load` / `skin_unload` / `skin_reload`（任意皮肤；目标是自己时 fire-and-forget——发起窗口随即销毁、返回值不可依赖，与右键菜单刷新/卸载同一教训）。⑤皮肤间事件总线 `skin_broadcast`（免权限）：向所有已加载皮肤派发 `desk-skin-message`（detail = channel/from/payload，channel 1–64 字符、payload ≤16KB；自己也收得到，自滤按 from）。`driftlet.js`/`driftlet.d.ts` 同步封装（theme 成员、httpRequest、hideSelf、broadcast、loadSkin/unloadSkin/reloadSkin、onThemeChanged/onSkinMessage 事件助手）。perms.js 分级表与 i18n 收录 `network`（高危红）。cargo test 110 过（新增桥烘焙主题测试）。双版开发指南（§2.3 权限表、§5.1 桥成员、§5 速查表、§5.3 新小节、§5.4 免权限小节）、双版 README、关键机制双版（八权限、无害命令四枚、主题烘焙与事件总线条目）、CHANGELOG 未发布节同步。
- **示例皮肤 `power-tools`**：file_system（高危）+ control（中危）两权限的演示皮肤（`examples/power-tools`，中英双语跟随管理器）——任意绝对路径文件读写卡（`skin_read_any_file` / `skin_write_any_file`，含二进制 base64 读取，失败 reject 系统错误原文）与皮肤窗口配置控制卡（`skin_get_window_config` 读取并 JSON 展示 / `skin_set_window_config` 非空字段组 patch：opacity、zoom、x/y、层级；默认目标为本皮肤，应用即见窗口变化）；权限 `file_system`（高危红）+ `control`（中危黄）。管理器同款现代材质（冷纸底 + 浮卡 + 软投影），桥缺失占位与语言跟随等全部皮肤规范遵守。node --check 与 pack-skin 校验通过；无头 Edge + iframe 真实 360×540 视口截图目检通过。双版开发指南 §10、双版 README 目录树、CHANGELOG 未发布节同步。
- **皮肤 API 新增两个高危权限与四条命令**：① `file_system`（文件读写）——`skin_read_any_file` / `skin_write_any_file`，任意**绝对路径**的文件读写（越过皮肤目录沙箱，整盘可达；只收绝对路径，失败一律透传系统错误原文，binary 按 base64 收发，沿用 32MB/16MB 上限，写入自动建父目录）。② `control`（皮肤控制）——`skin_get_window_config` / `skin_set_window_config`，读取/修改**任意皮肤**（含自己）的窗口配置项（opacity / placement / click_through / position_locked / resizable / zoom / edge_snap / snap_gap / x,y / width,height）；patch 按键部分更新，未知键/坏值整批拒绝，x/y 与 width/height 单边可取当前值，zoom 永远先于 size 应用；**修改逐项分发到管理器命令新抽取的 `set_skin_*_impl` 进程内实现**（十个管理器命令全部改为 require_manager 薄壳 + impl，持久化与运行态应用零抄录复用）。注意已取消的 `files` 权限名永不复活（旧皮肤残留声明会静默获得新语义的能力），任意路径读写故另起新名。安装引导与页眉胶囊的分级表同步收录两新权限（file_system 高危红、control 中危黄）。双版开发指南 §2.3/§5 速查表/§5.3 新小节、`driftlet.js`/`driftlet.d.ts` 封装与类型、双版 README、关键机制双版同步。
- **拖放安装 .dskin**：把皮肤包文件直接拖进管理器窗口即进入安装引导（拖入悬停时全窗虚线框遮罩反馈 `.drop-mask`）。与「+ 添加皮肤」/ 双击 `.dskin` 同一安装链路（向导自带校验、权限声明与状态确认），不新开第二条确认路径；经窗口级 `onDragDropEvent` 取真实路径（HTML5 drop 拿不到全路径，另加 `dragover`/`drop` 双 preventDefault 防 WebView2 就地导航）；非 `.dskin` 文件明确拒绝提示；一次拖入多个包入队逐个开引导页（引导页开着时入队不打断，关闭后继续）。关键机制双版入口条目同步。
- **完全出屏皮肤的自动找回 + 手动复位**：拔外接显示器 / DPI 拓扑变更后皮肤可能落在所有显示器工作区之外（不可见也拖不回）。新增 `factory::offscreen_target` 判定（窗口矩形与所有显示器工作区都不相交才算出屏，部分出屏视为合法摆放不动），5 秒维护定时器顺带救回（拓扑变化最迟 5 秒自愈，启动自载亦被首 tick 覆盖，无需 WM_DISPLAYCHANGE 钩子），移回主显示器工作区边缘并经 Moved 事件防抖持久化；配置页「操作」区新增「复位到屏幕内」按钮（`bring_skin_onscreen` 命令，`require_manager`，在屏时不动作并 toast 说明）。关键机制双版同步。
- **页眉卡权限名称胶囊**：皮肤的权限声明从安装引导的一次性展示变为装完随时可查——`SkinDetail` 透传 `permissions`，配置页页眉卡（皮肤名/元信息下）以名称胶囊呈现：高危红 / 中危黄 / 未知名灰，只列名称不带图标与详述（详述留在安装引导页），未声明时为中性说明行；展示按风险降序（高危最前、中危次之、未知名最后，同档保持声明顺序），引导页详细列表与页眉胶囊同序；两处共用新收编的 `src/js/perms.js` 单一口源（分级口径只维护一处）。关键机制双版、设计系统契约同步。
- **皮肤「已隐藏」状态徽标**：管理器皮肤卡片与配置页眉在「未加载（灰）/ 运行中（绿）」之外新增第三态「已隐藏」（黄色信号灯徽标，新增 `--warning-ring` 令牌双主题）。隐藏状态由后端按**真实窗口可见性**下发——`list_skins` / `get_skin_detail` 现算（已加载且 `is_visible()` 为假），不按热键按下与否簿记（托盘勾选、皮肤窗 Alt+F4 降级隐藏同样落真实状态）；前端刷新触发 = 新事件 `skins-visibility-changed`，发出点在 `sync_tray_toggle_item`（全部可见性变化的既有漏斗，三条隐藏路径都经过）。关键机制双版同步。

### 修复

- **全项目代码审计修复（高 3 / 中 27 / 低 70+，前后端并行审查 + 逐条亲验）**——①**高危**：`open_external` 可执行黑名单被尾点/尾空格绕过（`"RUN.EXE."` 扩展名解析为空串绕开黑名单、Windows 规范化后真执行——补分量尾点/尾空格拒绝 + 黑名单补齐 `.chm/.settingcontent-ms/.scf/.hlp/.wsc/.sct`）；边框拖拽尺寸永不落盘（换算尺取「当前物理÷已存逻辑」恒等旧值——改建窗时锁定的快照尺，与尺寸无关）；安装引导页卡片超高裁切（flex 居中缺安全居中——`.wizard-card{margin:auto}`）。②**中危安全簇**：`file_system` 组放行 UNC 路径（NTLM 外泄，与 open_external 拒 UNC 自相矛盾——双前缀 + Prefix 分量双道拦）；`http_request` 免权限 SSRF（可读 localhost/内网响应体——私有地址段全段拦截 + 重定向上限 3）；palette 预设色值未校验直进 style 属性（管理器窗口 CSS 注入——#hex 白名单）；安装 inspect→install TOCTOU（实装 id 与解析 id 不一致即回滚拆除）；manifest window 默认值不校验（可造隐形置顶全屏吃点击窗口——load 时统一钳制宽高/opacity/zoom/refresh_seconds）；同 id 不同文件夹名遮蔽（安装前拦下引导先移除）。③**中危正确性簇**：四处 `or_default()` 播种 300×200 覆盖 manifest 默认值（统一改 `runtime_entry_or_manifest`）；单边尺寸补丁 zoom≠1 双重缩小 + 单边位置跳 (0,0)（回退值改读当前实际几何）；网页皮肤刷新线程随 reload 泄漏叠加（闭包持有建窗句柄，旧窗销毁 eval 出错即退出）；`skin_read_file("CON")` 挂死主线程（DOS 设备名剥离扩展名匹配即拒）；`GetWindowTextW` 对假死窗口同步阻塞（改 SendMessageTimeoutW 300ms）；配置面板清空失焦把皮肤移去 (0,0)（NaN 不再兜底 0）；deepseek 换 Key 撞上在飞查询（fetchKey 校验 + abort）；向导「立即加载」失败卡片渲染进新向导（补代际守卫）；媒体音量回读竞态、sys-monitor 轮询在飞护栏、media-hub 防重入与切源竞态。④**hidden 复发两处**：media-hub 封面占位、power-tools 预览图的 `[hidden]` 被作者 display 顶掉（web-view 同款事故的第三、四处，全部显式压住，防护规则写入关键机制双版防第五处）。⑤**低危成批**：事件广播全部收窄 emit_to("main")；生命周期 load/unload/reload/reset 互斥锁（lifecycle_lock，锁序 lifecycle→install→settings 钉死）；NaN 穿透 clamp 三处口径统一（opacity/shell 超时/音量）；备份导出目标不得位于源目录内、junction 不跟随、条目/字节与导入侧同上限、临时文件 rename 就位、导入崩溃窗口启动回滚（.import-old 检测）；热重载自写登记 TTL 判有效（一次写多事件全挡）、监视根替换自愈重建、有界通道；托盘可见性漏斗过 load/reload 收尾、reload_all 快照拿锁后再取；热键回滚簿记按旧注册真实存活记账；截图零字节报错；pinner 值守线程 panic 防护；销毁路径 hwnd() 失败也摘净 HWND 键登记（pinner 存值兜底）；落盘换算尺取不到时跳过本次写盘；destroy 2s 饿死改报错；snap_gap 消费侧总闸钳制；超宽窗吸附候选剔除；5s 定时器 HWND 主线程现取；窗口图标按 DPI 分档缓存；inject_bridge 大小写不敏感定位（不 to_lowercase 防索引漂移）；包安装复制 junction 跳过；任意路径读文件 take 上限流式；通知与 GPU 查询挪 spawn_blocking；PDH 对齐 UB 改类型化缓冲；音频 COM 套间配对释放 + 重建锁内复查；app_log 100ms 合批；set_skin_zoom 失败回滚；前端控件族（主题连点串行化、导入成功保持禁用、日志清空代际、启动失败明示、级别原型键、向导换包 onClose 不丢、字体加载失败代际、位置尺寸回写焦点保护、设置保存失败空态保护、stepper 指数记数法、reload 防连点、Ctrl+F 弹层守卫、刷新失败保留旧列表与双 toast、更新检测关闭失败弹错）；示例皮肤族（toolbox 超时空值/引号参数/待办截断提示/焦点保护/files 误标、controls-demo 假数字、deepseek 隐藏暂停绕不过、power-tools 文案、web-view 双重加载与协议白名单）；CSS 族（日志下拉宽度、toast 断行、皮肤名/id 断行兜底、perm-chip 上限、向导权限标签断行、step-val 上限、弹层层级倒挂、死 class/死变量清理）。cargo test 113 过（新增 SSRF/设备名/自写 TTL 三用例）、vite build 通过。关键机制双版新增 hidden 防护条目；双版指南 password 口径修正（serve 时恒空、运行期管理器保存同步进本窗口——同域不越权但不得信任烘焙副本）。
- **层级「置顶 → 正常（贴桌面）」切换不再打断皮肤运行时**：该方向原先走整窗重建（reload），皮肤的 JS 运行时状态（计时器、进行中的任务）被重置。改为原位翻转——先 `set_always_on_top(false)`（tao 内部标志同步），再 `pinner.pin`（落 HWND_BOTTOM + 登记 250ms 值守环），终态与建窗时 pin 完全一致；Win+D「显示桌面」行为不变（值守环无切换状态机，认登记不认路径）。「正常 → 置顶」本就原位翻转，放置切换至此两向都不重建窗口、不发 loaded/unloaded 事件（面板选中态由点击处理就地迁移，无需联动刷新）；操作流水改由 `set_skin_placement` 单点记一行。关键机制双版同步。

## [1.0.8] - 2026-08-16

### 变更

- **五个演示皮肤 UI 统一现代化为管理器同款材质**：`controls-demo` / `media-hub` / `sys-monitor` / `toolbox` 四个浅色皮肤从「`#f4f6f8` 平涂 + 8px 圆角 + 3px 顶部色带 + 虚线分隔」的旧朴素样式接入管理器令牌语言（冷纸底 `#f4f8fb`、白色浮卡 + 双层软投影、圆角 12/16、hairline 取代虚线分隔）；应用栏撤 3px 顶部色带改 hero 晕染卡（`controls-demo` 的晕染经 `color-mix` 跟随「主题色」设置项，不写死蓝）；用量条/进度条铺品牌渐变、按钮改白面 chip（激活/主按钮渐变 + 蓝光晕，media-hub 播放/暂停升为主按钮）、sys-monitor 进程排序与 media-hub 频谱来源改现代分段控件（灰底容器 + 激活块）、sys-monitor 主读数走 Cascadia 等宽大数字、徽标改软色胶囊（免权限绿 / 桥缺失红 / 状态点带微光）、toolbox 输入改灰底嵌入 + 焦点蓝环；`controls-demo` 只留浅色：背景色调由「白天/夜晚/自动」改浅色三档（冷纸/暖白/雾蓝，夜晚与按时间自动取消，旧存值归一到默认档），互斥开关组控件演示保留，进度条与小秒表并为一行；`deepseek-balance` 由深色玻璃改浅色白卡（管理器同款冷纸语言：`#ffffff→#f2f7fc` 微渐变外壳、灰底嵌入构成行、徽标改软色胶囊、充值钮改主题色实心 + `color-mix` 光晕，`--accent` 主题色设置项保留）。布局结构与 JS 契约（全部 id、动态类名、`--accent` 变量挂点）零改动。无头 Edge + iframe 真实视口截图目检通过（340×560 controls 昼夜双档、340×480 media、340×540 sys、360×540 toolbox、300×200 balance；headless 窄窗「布局视口宽于截图宽度、右侧元素出画」的工具问题用 iframe 包裹规避）；双版开发指南 §10 措辞同步。

- **管理器 UI 材质现代化（纯 CSS，布局/标记零改动）**：原版「纯白平涂 + 发丝描边 + 小圆角」偏素显旧，本次只换材质不动结构——浅色底由纯白改冷纸色（`--bg-app #f4f8fb`，侧栏 `#eaf2f8`），卡片变白色浮面并启用环境软投影（新增 `--shadow-card` 双层：贴地接触影 + 环境光，`--shadow-chip` 给页签激活块等小浮起件），层级由明度 + 投影承担而非满屏 hairline（`--border` 同步减淡）；圆角放大一代（`--radius-lg 16 / --radius-md 12 / --radius-sm 8`，原 10/8/6）；深色主题改「暮海柔蓝」柔暗（dim）路线：刻意不走近黑 + 霓虹 accent 的同质答案，整体明度抬到中灰钢蓝区间（底 `#262e39` < 侧栏 `#222a34` < 浮卡 `#323c4b`），正文 ~88% 亮 `#dce3ea`（非纯白），描边 `rgba(170,200,225,…)`，accent 天蓝 `#63b8e6` 为唯一色相来源（浮卡上文本级 ~5:1），投影随面亮收敛；空态氛围光独立令牌 `--empty-glow`（双主题各档，解决图标背景光斑过强）。品牌色更敢用：主操作按钮（`.btn-add`/`.action-btn.primary`/`.confirm-btn.primary`/`.theme-btn.active`）加 `--accent-glow` 海浪蓝光晕投影；配置页眉改 hero 卡（`--header-wash` 淡蓝晕染 + 软投影，撤掉下缘虚线海图尺）；节标菱形与滑块菱形手柄改铺 `--accent-grad` 渐变（手柄加光晕）；页签/主题分段改现代分段控件（灰底容器 + 白色激活块浮起）；皮肤预览框加 `--preview-grad` 内渐变画布 + 细点阵纹理、占位图标着 accent 色；空态漂流瓶改白面浮砖并补中心氛围光（与安装引导页既有光晕同配方）；动作按钮默认态改白面 chip；toast/弹层/引导页图标砖随新圆角与阴影令牌。类名契约、`hover-ok` 门控、动效时长预算、菱形标绘语言全部保持；JS 与 Rust 净改动为零。无头 Edge 双主题截图目检通过（主界面填充态/空态/设置页，960×640），`docs/设计系统.md` 概念与验证工作流同步（另修正截图宽度为真实窗宽 960×640、补 `--user-data-dir` 无头静默退出坑）。

### 新增

- **皮肤库搜索**：侧栏头部右侧新增放大镜钮（`.search-toggle`，收起态零占位），点击或 `Ctrl/Cmd+F` 展开搜索行（`.sidebar-search`，与皮肤卡同一浮面材质），按显示名（当前语言）/ ID / 作者大小写不敏感即时过滤（列表规模小，无防抖）；**可收起设计**——有查询词时行强制保持展开，清空后 `Esc` 或再点按钮收起（不做 blur 自动收起：blur 时卡片还在原位，收起引起的行高变化会让 click 落到错误卡片上）；展开中放大镜钮转 accent 软底常亮。查询状态常驻 `SkinList.query`、展开态常驻 `App.searchOpen`（均不随外壳重建丢失），语言切换全量重绘后由 `app.js bindSearch` 恢复；清除钮（有内容时出现，点击后焦点回输入框）；`Ctrl/Cmd+F` 在输入控件内与设置弹层开着时不劫持；过滤中数量徽标显示「命中/总数」；零命中为搜索专属空态（放大镜 × 图标，与「还没有皮肤」区分）；选中皮肤被滤掉时保留选中态，配置面板不受影响。i18n 五键双语言（`list.searchToggle` / `list.searchPlaceholder` / `list.searchClear` / `list.noResults` / `list.noResultsHint`）；按压共享规则与 reduced-motion 块收编 `.search-toggle`。无头 Edge 双主题收起/展开两态截图目检通过；`docs/设计系统.md` 类名契约同步。

- **示例皮肤 `deepseek-balance`**：DeepSeek 账户余额自动查询（`examples/deepseek-balance`，联网皮肤参考实现）——皮肤页直接 `fetch` 官方 REST 接口 `GET https://api.deepseek.com/user/balance`（服务端返回 CORS 允许头）；官方鲸鱼 Logo 自带文字 SVG 裁切图标区、内联 `currentColor` 随主题反白；API Key 走 `password` 设置项经 `skin_get_setting` 读取；定时自动查询（间隔 1–1440 分钟可配，默认 30）+ 页面隐藏暂停 / 恢复可见即补查 + 手动刷新按钮；总余额与充值余额展示（赠金常规为 0，仅在存在时显示）；余额预警线（默认 5，任一币种跌入即标黄数字 + 徽标告警，0 关闭）与低余额 Windows 通知（边沿触发不刷屏、余额回升后重新武装）、页脚一键打开充值页（`open_external`，platform.deepseek.com/top_up）；正常 / 余额不足 / 查询失败 / 未配置四态徽标；主题色与充值余额开关实时应用，中英双语跟随管理器；权限 `system`（系统通知 + 打开外部链接，安装引导页标高危）。双版开发指南 §10 与双版 README 目录树同步。

### 修复

- **管理器/日志窗失焦后顶边 1px 描边消失（Win10）**：「无标题栏原生窗框」配方（`WS_THICKFRAME|WS_BORDER`、无 `WS_CAPTION`）下，DWM 只在窗口活动态画顶边描边——失焦时根本不画（探针实证失焦后窗口顶行像素=背景，不是画成浅色；该配方的帧外观完全由 `WM_NCACTIVATE` 的默认处理上报驱动）。`native_frame_proc` 现在拦截 `WM_NCACTIVATE`：原参数先放行给 tao 做焦点簿记（`set_active` + focus 事件不受影响），再对 `DefWindowProcW` 谎报 `(TRUE, -1)` 把帧外观钉在活动态，失焦后描边/阴影不再消失。新增探针 `tools/win32-probes/focus-frame-probe.ps1`（配方级三路对照：现行=失焦无描边、谎报=两态俱在、跳过 DefWindowProc 直接返回 1=连活动态描边一起丢掉）与 `manager-focus-frame.ps1`（真机管理器窗两态采样，含 `AttachThreadInput` 破前台锁手法；修复前复现、修复后两态俱在）。关键机制双版同步。

## [1.0.7] - 2026-08-14

### 变更

- **安装器三处文案/默认行为修正**（installer.nsi 自定义，中英双语同步）：①欢迎页删去「请先关闭其他所有应用程序」段落——本安装器只需重启 Driftlet 自身（appRunning 检查已覆盖），无更新系统文件之虞，经 `MUI_WELCOMEPAGE_TEXT` 换成自定义 `LangString driftletWelcomeText`；②完结页「开机自动启动」改为默认勾选（opt-out，不再读注册表现状作初值，Back→Next 重进仍恢复用户选择）；③检测到旧版的重装页选项互换——「覆盖安装」（原「请勿卸载」）升为默认首项，「安装前卸载」降为次项并标注「（数据将会清除）」，顶部推荐语同步改为「推荐直接覆盖安装」（仅升级分支；降级分支保留原版「推荐先卸载」文案、只互换选项，同版本分支顺序本就是保留数据项在前故不动，仅「卸载」标签同样标注「（数据将会清除）」）；`PageLeaveReinstall` 升级/降级分支第一选项语义随之反转为直接安装。关键机制双版同步。

### 修复

- **管理器/日志窗改为「无标题栏原生窗框」，Win10 重获原生边框+阴影**：无边框建窗后 `apply_native_frame` 剥 `WS_CAPTION`、加回 `WS_THICKFRAME|WS_BORDER`，并用 `native_frame_proc` 子类把 `WM_NCCALCSIZE` 绕开 tao 交还 `DefWindowProcW` 默认处理——关键坑是 tao 0.35 对 `decorations(false)` 窗口自建 NCCALCSIZE=0，DWM 据「非客户区为空」判定无框可画，只补样式位无效；第二坑是 tao 的 `apply_diff` 在任何窗口标志变化时（`show()`、最大化/还原等）全量重写 `GWL_STYLE` 并无条件带回 `WS_CAPTION`，故子类再拦 `WM_STYLECHANGING` 在样式落地前就地剥掉标题栏位。Win10 实机探针（新增 `tools/win32-probes/frame-probe.ps1`）逐一证伪候选路线：NCCALCSIZE 归零 → 边框阴影全丢；部分非客户区 → 画出完整标题栏；仅「THICKFRAME+BORDER、无 CAPTION、默认 NCCALCSIZE」= 原生 1px 边框 + 阴影 + 无标题栏（Win11 圆角/轮廓随真实窗框自动生效），`verify_manager_frame.py` 实测管理器窗 ext bounds 内缩 7px 确认 DWM 画框。随附拆除：custom caption 子类（`install_custom_caption`，NCCALCSIZE 归零路线）、早前已拆的 CSS 自绘描边不再恢复；两窗口保持 `shadow(false)`（玻璃延伸与真窗框叠加会留 1px 玻璃线）。皮肤窗口无边框机制不动。关键机制双版同步改写。
- **管理器/日志窗顶部 ~7px 死白边**（上一条「无标题栏原生窗框」落地的观感尾巴）：默认 `WM_NCCALCSIZE` 把可缩放边框厚度 inset 加在四边，左/右/下被 DWM 放到可视区外，唯独顶部留在可视区内——DWM 只画 1px 顶边框，其余 ~7px 是死白边，整个标题栏被压低。`native_frame_proc` 现在在默认处理后、非最大化时把 `rgrc[0].top` 改写为「窗口顶 + 1px」（最大化的处理见下一条），四边观感对齐为 1px 边框紧贴内容；新增探针 `tools/win32-probes/top-inset-probe.ps1` 实证 clientTop 8px→1px、边框+阴影完好、无标题栏。关键机制双版同步。
- **管理器/日志窗最大化变「全屏」盖住任务栏区**：无 `WS_CAPTION` 的 `THICKFRAME` 窗口最大化时系统按显示器全矩形摆窗（不做 work area 收缩），默认 NCCALCSIZE 得出的 client 精确等于显示器全尺寸，且窗口矩形底边非客户区把非置顶任务栏整段盖掉（tao 归零 NCCALCSIZE 的无边框时代同样有此问题）。`native_frame_proc` 双管齐下：`IsZoomed` 时把 `rgrc[0]` 整体改写为 `MonitorFromWindow` 的 `rcWork`（内容区贴合工作区），并拦 `WM_GETMINMAXINFO` 把 `ptMaxPosition`/`ptMaxSize` 改为「work area + 边框膨胀」（窗口矩形不再伸进任务栏区，膨胀量从默认 `ptMaxSize` 反推、跨 DPI，tao 的 min 约束不受影响）；探针 `top-inset-probe.ps1` 的 M-zoomed 案实证最大化 rect=work+膨胀、client=rcWork、DWM 不画标题栏。关键机制双版「第三个坑」同步扩写。

### 移除

- **安装向导的免声明能力清单行**：撤下 1.0.6 新增的权限区块后固定提示（`wizard.freeCapabilities` 中英键、`.wizard-freecaps` 样式一并摘除），权限展示回到仅逐条列出声明项 + 风险分级。双版指南 §2.3 同步。

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
