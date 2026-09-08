// 屿 · 时钟（isles-clock）：指针式表盘，时/分/秒三针 + 圆点时刻。零权限，零文字。
// M 档 200×200 单形态（1×1 足够——时钟不看长文）。rAF 逐帧驱动：秒针平滑
// 扫秒（墙钟含毫秒）；系统「减弱动画」开启时秒针退化为每秒步进。
// 页面隐藏停帧、唤回即续。主题预设写 bg/指针/秒针三色，单项可再微调。
(() => {
  const $ = (id) => document.getElementById(id);

  // ── 圆点时刻：12 枚，整点（12/3/6/9）加大加深（极简圆点盘——无盘面圈、
  //    无刻度线；盘面圈会读成进度环的轨道，是本皮的刻意规避） ──
  function buildDots() {
    const NS = "http://www.w3.org/2000/svg";
    const g = $("dots");
    for (let k = 0; k < 12; k++) {
      const major = k % 3 === 0;
      const a = (k * Math.PI) / 6;
      const c = document.createElementNS(NS, "circle");
      c.setAttribute("cx", 60 + 50 * Math.sin(a));
      c.setAttribute("cy", 60 - 50 * Math.cos(a));
      c.setAttribute("r", major ? 2.6 : 1.8);
      c.setAttribute("class", major ? "dot dot-major" : "dot");
      g.appendChild(c);
    }
  }

  // ── 主题预设（写 bg/指针/秒针色三项，单项可再微调；自定义 = 不动） ──
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
    if (!t || !invoke) return;   // 未知主题名 = 不动当前颜色
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

  // ── 逐帧驱动：秒针平滑扫秒（毫秒入角）；「减弱动画」开启时秒针每秒步进。
  //    MQL 缓存一次（每帧新建 matchMedia 对象是浪费）；角度量化到 0.01°、
  //    变了才写——秒针照常逐帧扫，时/分针的无效样式写入全省 ──
  const mq = window.matchMedia ? window.matchMedia("(prefers-reduced-motion: reduce)") : null;
  let raf = 0;
  let lastS = "", lastM = "", lastH = "", lastLabel = "";
  function frame() {
    const now = new Date();
    const sec = now.getSeconds() + (mq?.matches ? 0 : now.getMilliseconds() / 1000);
    const min = now.getMinutes() + sec / 60;
    const hour = (now.getHours() % 12) + min / 60;
    const ds = (sec * 6).toFixed(2), dm = (min * 6).toFixed(2), dh = (hour * 30).toFixed(2);
    if (ds !== lastS) { lastS = ds; $("hand-s").style.transform = `rotate(${ds}deg)`; }
    if (dm !== lastM) { lastM = dm; $("hand-m").style.transform = `rotate(${dm}deg)`; }
    if (dh !== lastH) { lastH = dh; $("hand-h").style.transform = `rotate(${dh}deg)`; }

    // 屏读器 aria-label：分钟粒度更新（表盘本体是图，文字时间给读屏）
    const label = `${Isles.pad2(now.getHours())}:${Isles.pad2(now.getMinutes())}`;
    if (label !== lastLabel) {
      lastLabel = label;
      $("dial").setAttribute("aria-label", Isles.t(`当前时间 ${label}`, `Current time ${label}`));
    }
    raf = requestAnimationFrame(frame);
  }
  function start() { if (!raf) raf = requestAnimationFrame(frame); }
  function stop() { cancelAnimationFrame(raf); raf = 0; }
  document.addEventListener("visibilitychange", () => (document.hidden ? stop() : start()));

  // ── 接线与启动 ──
  document.addEventListener("desk-setting-changed", (e) => {
    const { key, value } = e.detail || {};
    if (key === "theme") applyTheme(value);
    applyStatic();
  });
  document.addEventListener("desk-language-changed", () => { lastLabel = ""; applyStatic(); });

  buildDots();
  applyStatic();
  start();
})();
