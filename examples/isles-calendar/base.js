/* ==========================================================================
   屿 · Isles —— 共享运行时小件（window.Isles）
   唯一事实源：examples/shared/base.js
   全部接口做无桥兜底——皮肤拖进浏览器裸开也能渲染（宿主调试约定）。
   ========================================================================== */
window.Isles = (() => {
  const bridge = () => window.driftlet || window.__DESK_PP__ || null;

  /** 界面语言：'zh' | 'en'（跟随管理器） */
  const lang = () =>
    ((bridge()?.language || "zh-CN").toLowerCase().startsWith("en") ? "en" : "zh");

  /** 双语取词：Isles.t('中文', 'English') */
  const t = (zh, en) => (lang() === "en" ? en : zh);

  /** 皮肤设置当前值（key → value 对象，页面加载前已注入） */
  const settings = () => bridge()?.settings || {};

  /* 调色板控件产出 #rrggbb 或 #rrggbbaa（带透明度）——两种都收 */
  const HEX = /^#([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;

  /** 把用户调色板选的 accent 落到 CSS 变量（空/非法值保持默认） */
  const setAccent = (hex) => {
    if (typeof hex === "string" && HEX.test(hex)) {
      document.documentElement.style.setProperty("--isles-accent", hex);
    }
  };

  /** 场景/图片卡文字主色（深浅副色自动派生）；非法值回落默认白族 */
  const setTextColor = (el, hex) => {
    if (!el) return;
    if (typeof hex === "string" && HEX.test(hex)) el.style.setProperty("--isles-on-photo", hex);
    else el.style.removeProperty("--isles-on-photo");
  };

  /** 定向暗纱色相（压底渐变的颜色，其 alpha 即浓度——四挡比例恒定保
      形态，透明度拉到最左 = 关闭暗纱）；非法值回落默认深色 */
  const setScrimColor = (el, hex) => {
    if (!el) return;
    if (typeof hex === "string" && HEX.test(hex)) el.style.setProperty("--isles-scrim-color", hex);
    else el.style.removeProperty("--isles-scrim-color");
  };

  /** 背景高斯模糊（px，0 = 不模糊；只糊 .isles-bg 图层，文字不受影响。
      落整段 blur() 字符串：blur(0px) 也会强制走滤镜合成路径，0 必须真移除） */
  const setBgBlur = (el, px) => {
    if (!el) return;
    const n = Number(px);
    if (Number.isFinite(n) && n > 0) el.style.setProperty("--isles-bg-blur-filter", `blur(${Math.min(40, Math.round(n))}px)`);
    else el.style.removeProperty("--isles-bg-blur-filter");
  };

  /** 注册/更新皮肤右键菜单的自定义项（≤8 条；id 小写连字符 ≤32；label /
      label_zh / label_en 至少其一；checked 勾选态；传 [] 清除）。
      点选经 desk-skin-menu-item 事件回投（见 onMenuItem） */
  const setMenuItems = (items) =>
    bridge()?.invoke?.("skin_set_menu_items", { items })?.catch(() => {});

  /** 右键菜单自定义项点选：Isles.onMenuItem(id => ...) */
  const onMenuItem = (fn) =>
    document.addEventListener("desk-skin-menu-item", (e) => fn(e.detail?.id));

  const pad2 = (n) => String(n).padStart(2, "0");

  /** 'YYYY-MM-DD' → 本地零点 Date（非法返回 null） */
  const parseDay = (s) => {
    const m = typeof s === "string" && s.match(/^(\d{4})-(\d{2})-(\d{2})/);
    if (!m) return null;
    const d = new Date(+m[1], +m[2] - 1, +m[3]);
    // JS 日期翻滚：「2025-02-31」会静默滚成 3 月 3 日且 isNaN 查不出——
    // 回读月/日核对，翻滚即非法（审查发现：宿主只查日 1–31）
    if (isNaN(d) || d.getMonth() !== +m[2] - 1 || d.getDate() !== +m[3]) return null;
    return d;
  };

  /** 目标日的整天差：>0 未来还有 N 天，<0 已过 N 天，0 今天 */
  const daysUntil = (dayStr) => {
    const target = parseDay(dayStr);
    if (!target) return null;
    const now = new Date();
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    return Math.round((target - today) / 86400000);
  };

  /** 跨零点重算（倒数类皮肤用）：回调注册一次即可 */
  const onMidnight = (fn) => {
    const arm = () => {
      const now = new Date();
      const next = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1, 0, 0, 2);
      setTimeout(() => { fn(); arm(); }, next - now);
    };
    arm();
  };

  return { bridge, lang, t, settings, setAccent, setTextColor, setScrimColor, setBgBlur, setMenuItems, onMenuItem, pad2, parseDay, daysUntil, onMidnight };
})();
