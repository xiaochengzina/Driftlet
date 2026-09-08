// 屿 · 日历（isles-calendar）：左区日期信息 + 右区月历。零权限。
// 农历与节日由 vendor/lunar.min.js 提供（lunar-javascript 1.7.7，MIT 许可，
// 许可文本见 vendor/LICENSE.txt）——成熟实现，不自己造轮子。
// 月历交互：点选其他日期 → 左区换为该日信息（再点该日或点今天回到今天）；
// 点首尾的邻月日 = 翻到该月并选中；‹ › 翻月；点月份标题回本月今天。
// 视图月与点选状态都是会话内存、不持久化（重载即回本月今天）。
// 跨零点经 Isles.onMidnight 重渲染。
(() => {
  const $ = (id) => document.getElementById(id);
  const SIZES = { mini: [200, 200], wide: [420, 200] };
  const WEEK_ZH = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
  const WEEK_ZH_FULL = ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"];
  const WEEK_EN = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  const WEEK_EN_FULL = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
  const MON_EN = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const WHEAD_ZH = ["一", "二", "三", "四", "五", "六", "日"];
  const WHEAD_EN = ["M", "T", "W", "T", "F", "S", "S"];
  // 周日开头变体（「周起始」设置项切换）
  const WHEAD_ZH_SUN = ["日", "一", "二", "三", "四", "五", "六"];
  const WHEAD_EN_SUN = ["S", "M", "T", "W", "T", "F", "S"];
  const sunFirst = () => Isles.settings().week_start === "sun";

  // ── 月历视图状态（会话内存，不持久化） ──
  let viewY = null, viewM = 0;   // 当前翻到的月份
  let selKey = null;             // 点选日期 "YYYY-M-D"；null = 今天

  const keyOf = (d) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
  function parseKey(k) {
    const m = /^(\d+)-(\d+)-(\d+)$/.exec(k || "");
    return m ? new Date(+m[1], +m[2], +m[3]) : null;
  }

  // ── 尺寸档（与设置面板/右键菜单同一设置项；加载不回写） ──
  function applySize(size) {
    const wh = SIZES[size] || SIZES.wide;
    Isles.bridge()?.invoke?.("skin_set_window_config", { patch: { width: wh[0], height: wh[1] } })?.catch(() => {});
  }
  function currentSize() {
    const v = Isles.settings().size;
    return SIZES[v] ? v : "wide";
  }
  function syncMenu(current) {
    const cur = current || currentSize();
    Isles.setMenuItems([
      { id: "wide", label_zh: "横条 2×1", label_en: "Wide 2×1", checked: cur === "wide" },
      { id: "mini", label_zh: "小方 1×1", label_en: "Mini 1×1", checked: cur === "mini" },
    ]);
  }
  Isles.onMenuItem((id) => {
    if (!SIZES[id]) return;
    Isles.bridge()?.invoke?.("skin_set_setting", { key: "size", value: id })?.catch(() => {});
    applySize(id);
    syncMenu(id);
  });

  // ── 主题预设（写 bg/text/accent 三项；自定义 = 不动） ──
  const THEMES = {
    // 族主题四套（浅 3 + 暗 1；青柠为基准不调）
    sora:     { bg: "#F0F6FC", text: "#17334E", accent: "#2C82E4" },
    lime:     { bg: "#F1FAF3", text: "#1E4030", accent: "#17B978" },
    lavender: { bg: "#F6F1FC", text: "#382D52", accent: "#7D5BE7" },
    nord:     { bg: "#1C2230", text: "#E7EEF7", accent: "#7FDCC9" },
  };
  const THEME_KEYS = { bg: "bg_color", text: "text_color", accent: "accent" };
  async function applyTheme(name) {
    const t = THEMES[name];
    const invoke = Isles.bridge()?.invoke;
    if (!t || !invoke) return;
    for (const [prop, hex] of Object.entries(t)) {
      await invoke("skin_set_setting", { key: THEME_KEYS[prop], value: hex }).catch(() => {});
    }
    // 主题色本地先行落 DOM（异步回同步竞态——见 applyStatic 注释）
    const over = {};
    for (const [prop, hex] of Object.entries(t)) over[THEME_KEYS[prop]] = hex;
    applyStatic(over);
  }

  const HEX = /^#([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;
  const setVar = (el, name, hex) => {
    if (typeof hex === "string" && HEX.test(hex)) el.style.setProperty(name, hex);
  };
  function applyStatic(over) {
    // over = 本地刚写入的主题色覆盖——桥内 settings 的静默同步是异步 eval，
    // 此刻回读会拿旧值（六皮同款竞态，审查应修）；事件流随后对齐
    const s = over ? { ...Isles.settings(), ...over } : Isles.settings();
    const card = $("card");
    setVar(card, "--isles-card-bg", s.bg_color);
    setVar(card, "--isles-fg", s.text_color);
    Isles.setAccent(s.accent);
  }

  // ── 农历/节日（左区信息随点选日期走） ──
  function infoOf(date) {
    const lu = Lunar.fromDate(date);
    const fests = [...lu.getFestivals(), ...Solar.fromDate(date).getFestivals()];
    return { lu, fests, jieqi: lu.getJieQi() };
  }

  // ── 月历网格（默认周一开头，可切周日开头；6×7 格含邻月首尾；英文界面不打节日标记） ──
  function buildGrid(now) {
    const en = Isles.lang() === "en";
    const todayKey = keyOf(now);
    const first = new Date(viewY, viewM, 1);
    const sun = sunFirst();
    const startCol = sun ? first.getDay() : (first.getDay() + 6) % 7;   // 周一 = 0 / 周日 = 0
    const isWeekendCol = (i) => (sun ? (i % 7 === 0 || i % 7 === 6) : i % 7 >= 5);   // 周末列弱化（两档不同列）
    const grid = $("grid");
    grid.textContent = "";
    for (let i = 0; i < 42; i++) {
      const d = new Date(viewY, viewM, 1 + (i - startCol));   // JS 自动滚月
      const other = d.getMonth() !== viewM;
      const k = keyOf(d);
      const { fests } = infoOf(d);
      // 格子必须是 button——桥的拖动接管只豁免 button/input 等交互元素，
      // div 格子的 click 会被 start_skin_drag 吃掉（实机点选失灵的事故）
      const cell = document.createElement("button");
      cell.type = "button";
      cell.className = "cell";
      cell.classList.toggle("other", other);
      cell.classList.toggle("today", k === todayKey);
      cell.classList.toggle("sel", selKey === k);
      cell.classList.toggle("weekend", isWeekendCol(i));   // 周末列弱化
      if (!en && fests.length) cell.classList.add("fest-day");
      const dateLabel = en ? `${MON_EN[d.getMonth()]} ${d.getDate()}` : `${d.getMonth() + 1}月${d.getDate()}日`;
      cell.setAttribute("aria-label", dateLabel + (!en && fests.length ? ` ${fests.join(" ")}` : "") + (k === todayKey ? (en ? " · today" : " · 今天") : ""));
      cell.title = en ? "" : fests.join(" · ");
      const num = document.createElement("span");
      num.textContent = d.getDate();
      cell.appendChild(num);
      cell.addEventListener("click", () => onPick(d, other));
      grid.appendChild(cell);
    }
    $("mtitle").textContent = Isles.t(`${viewY}年${viewM + 1}月`, `${MON_EN[viewM]} ${viewY}`);
  }

  // 翻月过场：方向性滑入（下一月从右来），「减弱动画」开启时瞬切
  const mq = window.matchMedia ? window.matchMedia("(prefers-reduced-motion: reduce)") : null;
  function flipGrid(dir) {
    if (mq?.matches) return;
    const grid = $("grid");
    if (typeof grid.animate !== "function") return;
    grid.animate(
      [{ opacity: 0, transform: `translateX(${dir > 0 ? 10 : -10}px)` }, { opacity: 1, transform: "none" }],
      { duration: 170, easing: "ease-out" }
    );
  }

  // 点选：邻月日 = 翻到该月并选中；今天 = 回到今天；再点已选 = 取消回今天
  function onPick(d, other) {
    let dir = 0;
    if (other) {
      dir = d > new Date(viewY, viewM, 1) ? 1 : -1;
      viewY = d.getFullYear();
      viewM = d.getMonth();
    }
    const k = keyOf(d);
    selKey = (k === keyOf(new Date()) || k === selKey) ? null : k;
    render();
    if (dir) flipGrid(dir);
  }

  function navMonth(dir) {
    const d = new Date(viewY, viewM + dir, 1);
    viewY = d.getFullYear();
    viewM = d.getMonth();
    render();
    flipGrid(dir);
  }

  // 回本月今天：选中与视图月一起复位（实机反馈——只清选中，月历还停在
  // 翻走的月份）；视图跨月才播方向性过场
  function resetToToday() {
    const now = new Date();
    const before = new Date(viewY, viewM, 1);
    const target = new Date(now.getFullYear(), now.getMonth(), 1);
    const dir = before < target ? 1 : before > target ? -1 : 0;
    viewY = now.getFullYear();
    viewM = now.getMonth();
    selKey = null;
    render();
    if (dir) flipGrid(dir);
  }

  // ── 渲染 ──
  function render() {
    const now = new Date();
    if (viewY === null) { viewY = now.getFullYear(); viewM = now.getMonth(); }
    // 与 CSS 容器查询同尺：@container 按内容盒判定，JS 这边 clientWidth 含
    // padding——窗宽 241–272px 死区里两边会分叉（实机可达：开拖拽缩放拖进
    // 该区间）。减掉横向 padding 对齐内容盒
    const card = $("card");
    const padX = parseFloat(getComputedStyle(card).paddingLeft) + parseFloat(getComputedStyle(card).paddingRight);
    const mini = (card.clientWidth - padX) <= 240;
    const shown = (!mini && selKey && parseKey(selKey)) || now;   // M 档恒看今天
    const { lu, fests, jieqi } = infoOf(shown);
    // 英文界面：不显示农历/节气，节日全部不显示（实机反馈——lunar 与中式
    // 节日对英文用户是噪声；星期与日期照常本地化）
    const en = Isles.lang() === "en";

    // 左区（P）
    $("ym").textContent = en
      ? `${MON_EN[shown.getMonth()]} ${shown.getFullYear()}`
      : `${shown.getFullYear()}年${shown.getMonth() + 1}月`;
    $("day").textContent = shown.getDate();
    $("lunar").textContent = en
      ? WEEK_EN_FULL[shown.getDay()]
      : `${WEEK_ZH_FULL[shown.getDay()]} · ${lu.getMonthInChinese()}月${lu.getDayInChinese()}${jieqi ? ` · ${jieqi}` : ""}`;
    $("fest").textContent = en ? "" : fests.join(" · ");

    // M 档顶/底悬浮行（恒为今天；英文底行留空——.mbot:empty 真移除）
    const t = infoOf(now);
    $("mtop").textContent = en
      ? `${WEEK_EN[now.getDay()]}, ${MON_EN[now.getMonth()]} ${now.getDate()}`
      : `${now.getMonth() + 1}月${now.getDate()}日 ${WEEK_ZH[now.getDay()]}`;
    $("mbot").textContent = en ? "" : (t.fests.length ? t.fests.join(" · ") : `${t.lu.getMonthInChinese()}月${t.lu.getDayInChinese()}${t.jieqi ? ` · ${t.jieqi}` : ""}`);
    $("mbot").style.color = t.fests.length && !en ? "var(--isles-accent)" : "";

    // 「回今天」回程链 + 翻月钮文案（i18n）；has-sel 只在横条档加（M 档恒今天）
    $("card").classList.toggle("has-sel", !!selKey && !mini);
    $("back-today").textContent = Isles.t("回今天", "Back to today");
    $("prev").title = Isles.t("上个月", "Previous month");
    $("prev").setAttribute("aria-label", $("prev").title);
    $("next").title = Isles.t("下个月", "Next month");
    $("next").setAttribute("aria-label", $("next").title);

    // 星期头（随语言与周起始重建）
    const whead = $("whead");
    whead.textContent = "";
    const heads = Isles.lang() === "en"
      ? (sunFirst() ? WHEAD_EN_SUN : WHEAD_EN)
      : (sunFirst() ? WHEAD_ZH_SUN : WHEAD_ZH);
    heads.forEach((w) => {
      const s = document.createElement("span");
      s.textContent = w;
      whead.appendChild(s);
    });

    buildGrid(now);
  }

  // ── 接线与启动 ──
  $("prev").addEventListener("click", () => navMonth(-1));
  $("next").addEventListener("click", () => navMonth(1));
  $("back-today").addEventListener("click", resetToToday);
  $("mtitle").addEventListener("click", resetToToday);   // 点标题同回本月今天
  window.addEventListener("resize", render);

  document.addEventListener("desk-setting-changed", (e) => {
    const { key, value } = e.detail || {};
    if (key === "size") applySize(value);
    if (key === "theme") applyTheme(value);
    applyStatic();
    render();
    syncMenu();
  });
  document.addEventListener("desk-language-changed", render);

  applyStatic();
  render();
  syncMenu();
  Isles.onMidnight(render);
})();
