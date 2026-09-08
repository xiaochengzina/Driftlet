// 屿 · 系统占用（isles-monitor）：左环（CPU/GPU/内存三环同心，分项占比）
// + 右系统整体占用热力图（GitHub 贡献图式，走势）。零文字皮肤。
// 权限 sys_info（低危只读）；1s 轮询、页面隐藏暂停。
// 结构 = 槽位（外/中/内）× 指标（cpu/gpu/mem）——环色跟槽位，数据跟指标；
// 槽位重复时按「用户新选的槽位赢、占用者换到被置换的原值」自动对调。
(() => {
  const $ = (id) => document.getElementById(id);
  const SIZES = { mini: [200, 200], wide: [420, 200] };
  const SLOTS = ["outer", "mid", "inner"];

  // GPU 数据源：多适配器（核显/独显）时按 gpu_adapter 设置的 LUID 精确
  // 绑定；空串 = 自动——独显优先（游戏/渲染机器上核显常占枚举首项、
  // 但它的占用不是用户想看的），无独显回退枚举首项；指定值失配
  //（拔出/禁用/驱动更新换 LUID）同口径回退独显优先
  function pickGpu(gpus) {
    const list = Array.isArray(gpus) ? gpus : [];
    const discrete = () => list.find((g) => g?.gpu_type === "discrete") ?? list[0] ?? null;
    const sel = (Isles.settings().gpu_adapter || "").trim();
    if (sel) return list.find((g) => g?.luid === sel) ?? discrete();
    return discrete();
  }

  const METRICS = {
    cpu: { zh: "CPU", en: "CPU", get: (d) => d.cpu?.[0]?.usage ?? null },
    gpu: { zh: "GPU", en: "GPU", get: (d) => pickGpu(d.gpus)?.usage ?? null },
    mem: { zh: "内存", en: "Memory", get: (d) => d.mem?.ram?.usage_pct ?? null },
  };
  const R = { outer: 50, mid: 38, inner: 26 };          // 环半径（viewBox 120）：环间距 6px、簇向内收
  const C = (r) => 2 * Math.PI * r;

  const HEX = /^#([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;
  const setVar = (el, name, hex) => {
    if (typeof hex === "string" && HEX.test(hex)) el.style.setProperty(name, hex);
  };

  // ── 尺寸档（与设置面板/右键菜单同一设置项） ──
  function applySize(size) {
    const wh = SIZES[size] || SIZES.wide;
    const invoke = Isles.bridge()?.invoke;
    if (!invoke) return;
    invoke("skin_set_window_config", { patch: { width: wh[0], height: wh[1] } }).catch(() => {});
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
    Isles.bridge()?.invoke?.("skin_set_setting", { key: "size", value: id }).catch(() => {});
    applySize(id);
    syncMenu(id);
  });

  // ── 环序：读设置（非法值兜底）+ 交换去重 ──
  let ringState = null;   // { outer, mid, inner } 当前有效环序

  function readRingState() {
    const s = Isles.settings();
    const v = (x, d) => (METRICS[x] ? x : d);
    return { outer: v(s.outer, "mem"), mid: v(s.mid, "cpu"), inner: v(s.inner, "gpu") };
  }

  // 启动自检：存量重复值（或手工编辑 settings.json 的脏值）修正——后到槽位
  // 换成缺失指标，并回写设置项让面板一致
  async function reconcileRingState() {
    const cur = readRingState();
    const fixed = { ...cur };
    for (let i = 1; i < SLOTS.length; i++) {
      if (SLOTS.slice(0, i).some((k) => fixed[k] === fixed[SLOTS[i]])) {
        const present = new Set(SLOTS.map((k) => fixed[k]));
        fixed[SLOTS[i]] = Object.keys(METRICS).find((m) => !present.has(m));
      }
    }
    for (const k of SLOTS) {
      if (fixed[k] !== cur[k]) {
        await Isles.bridge()?.invoke?.("skin_set_setting", { key: k, value: fixed[k] })?.catch(() => {});
      }
    }
    ringState = fixed;
  }

  // 用户改某槽位：该槽位赢；占用同一指标的其他槽位换到该槽原值（对调）
  async function onRingKeyChange(key, value) {
    if (!METRICS[value] || !ringState) return;
    const displaced = ringState[key];
    for (const k of SLOTS) {
      if (k !== key && ringState[k] === value) {
        ringState[k] = displaced;
        await Isles.bridge()?.invoke?.("skin_set_setting", { key: k, value: displaced })?.catch(() => {});
      }
    }
    ringState[key] = value;
    // 热力图与环序无关（整体均值），无需重建
  }

  // ── 主题预设：一键写进五个颜色设置（面板同步），随后单项可再微调；
  // 文字色无控件——零值格轨道按背景明暗自动派生 ──
  const THEMES = {
    // 族主题四套（浅 3 + 暗 1；青柠为基准不调）。环色组沿用相邻色相拉开的
    // 五色策展——例外：澄空的中环用天青（偏绿的青 #12B5B0 在蓝纸上读割裂，
    // 实机反馈；A/B 目检 蓝→天青→紫 同冷族最谐）
    lime:     { bg: "#F1FAF3", outer: "#17B978", mid: "#8AC926", inner: "#2FA8C9", heat: "#17B978" },
    sora:     { bg: "#F0F6FC", outer: "#2C82E4", mid: "#2FA3D8", inner: "#7A6FE8", heat: "#2C82E4" },
    lavender: { bg: "#F6F1FC", outer: "#7D5BE7", mid: "#C084D8", inner: "#F472A8", heat: "#7D5BE7" },
    nord:     { bg: "#1C2230", outer: "#7FE0B8", mid: "#5ED4D4", inner: "#8DA4F0", heat: "#7FDCC9" },
  };
  const THEME_KEYS = { bg: "bg_color", outer: "outer_color", mid: "mid_color", inner: "inner_color", heat: "heat_color" };

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

  // ── 外观（方向/背景/环色/热力色；无文字控件——皮肤零文字，
  // --isles-fg 只剩热力图零值格淡轨道一个用途：按背景明暗自动派生） ──
  function applyStatic(over) {
    // over = 本地刚写入的主题色覆盖——桥内 settings 的静默同步是异步 eval，
    // 此刻回读会拿旧值（六皮同款竞态，审查应修）；事件流随后对齐
    const s = over ? { ...Isles.settings(), ...over } : Isles.settings();
    const card = $("card");
    card.classList.toggle("dir-right", s.direction === "right");
    setVar(card, "--isles-card-bg", s.bg_color);
    // 背景亮度派生轨道色：浅底用深轨、深底用浅轨（感知亮度阈值 160）
    const hex = HEX.test(s.bg_color || "") ? s.bg_color : "#F1FAF3";
    const r = parseInt(hex.slice(1, 3), 16), g = parseInt(hex.slice(3, 5), 16), b = parseInt(hex.slice(5, 7), 16);
    card.style.setProperty("--isles-fg", (0.2126 * r + 0.7152 * g + 0.0722 * b) > 160 ? "#1E2430" : "#E9EEF5");
    setVar(card, "--ring-outer", s.outer_color);
    setVar(card, "--ring-mid", s.mid_color);
    setVar(card, "--ring-inner", s.inner_color);
    setVar(card, "--heat-color", s.heat_color);
  }

  // ── SVG 环（建一次，轮询只改 dashoffset） ──
  function buildRings() {
    const NS = "http://www.w3.org/2000/svg";
    const g = $("ring-svg");
    g.textContent = "";
    for (const slot of SLOTS) {
      const r = R[slot];
      const track = document.createElementNS(NS, "circle");
      track.setAttribute("cx", 60); track.setAttribute("cy", 60); track.setAttribute("r", r);
      track.setAttribute("class", `ring-track track-${slot}`);
      const val = document.createElementNS(NS, "circle");
      val.setAttribute("cx", 60); val.setAttribute("cy", 60); val.setAttribute("r", r);
      val.setAttribute("class", `ring-val slot-${slot}`);
      val.style.strokeDasharray = `${C(r)}`;
      val.style.strokeDashoffset = `${C(r)}`;
      val.dataset.slot = slot;
      g.appendChild(track);
      g.appendChild(val);
    }
  }

  // ── 系统整体占用热力图（GitHub 贡献图式：7 行 × N 列、每格 = 30s 均值、
  // 按列推进、最新在右下；整体值 = cpu/gpu/mem 有效项均值） ──
  const HEAT_ROWS = 7;
  const HEAT_CELL = 14, HEAT_GAP = 4;
  let heatCells = [];
  let heatBuf = [];
  let heatAcc = [];                // 当前格的样本累计器（窗口长度 = 每格秒数设置）

  function buildHeat() {
    const box = $("heat");
    // 列数内缩一列：最右列不贴区缘，与左侧环区成对称留白（辅助线对位）
    const cols = Math.max(8, Math.floor((box.clientWidth + HEAT_GAP) / (HEAT_CELL + HEAT_GAP)) - 1);
    const len = HEAT_ROWS * cols;
    box.textContent = "";
    heatCells = [];
    for (let i = 0; i < len; i++) {
      const c = document.createElement("div");
      c.className = "heat-cell";
      box.appendChild(c);
      heatCells.push(c);
    }
    heatBuf = heatBuf.slice(-len);
    while (heatBuf.length < len) heatBuf.unshift(0);
    drawHeat();
  }

  // 值 → 档色：0 = 前景淡轨道，>0 按五档加深（GitHub Less→More 语义；
  // 刻度 10/25/40/60——桌面占用常年 10–40%，低档区加密；档间色距 ~18% 肉眼可分）
  function heatColor(v) {
    if (!Number.isFinite(v) || v <= 0) return "color-mix(in srgb, var(--isles-fg) 7%, transparent)";
    const lv = v < 10 ? 24 : v < 25 ? 40 : v < 40 ? 58 : v < 60 ? 76 : 94;
    return `color-mix(in srgb, var(--heat-color, #12B886) ${lv}%, var(--isles-card-bg))`;
  }

  // previewV：活格预览——尾格覆盖渲染当前 30s 窗口的运行均值（未提交的窗口
  // 也有实时反馈，提交后自然被均值定格）
  function drawHeat(previewV) {
    for (let i = 0; i < heatCells.length; i++) {
      heatCells[i].style.background = heatColor(heatBuf[i]);
    }
    if (previewV != null && heatCells.length) {
      heatCells[heatCells.length - 1].style.background = heatColor(previewV);
    }
  }

  // ── 轮询（1s；页面隐藏暂停，恢复立即补一拍） ──
  let timer = null;
  const _warned = new Set();
  function warnOnce(what, e) {
    if (_warned.has(what)) return;
    _warned.add(what);
    Isles.bridge()?.invoke?.("skin_log", { level: "warn", message: `${what} 查询失败（后续失败不再重复记录）: ${e}` })?.catch(() => {});
  }

  async function poll() {
    const invoke = Isles.bridge()?.invoke;
    if (!invoke || !ringState) return;
    let data = {};
    // 查询失败留痕（GPU/CPU 利用率在部分机器上间歇不可用——失败拍按 0 入窗，
    // 滚窗会被 0 主导、曲线贴底；日志窗口据此可判「数据年轻 vs 查询失败」）
    try { data.cpu = await invoke("get_cpu_info"); } catch (e) { warnOnce("cpu", e); }
    try { data.gpus = await invoke("get_gpu_info"); } catch (e) { warnOnce("gpu", e); }
    try { data.mem = await invoke("get_memory_info"); } catch (e) { warnOnce("mem", e); }

    document.querySelectorAll("#ring-svg .ring-val").forEach((el) => {
      const m = METRICS[ringState[el.dataset.slot]];
      const pct = m?.get(data);
      const r = Number(el.getAttribute("r"));
      el.style.strokeDashoffset = pct == null ? `${C(r)}` : `${C(r) * (1 - Math.max(0, Math.min(100, pct)) / 100)}`;
    });

    // 整体占用 = 三项有效值的加权均值（权重 0–10；权重全 0 或无有效值按 0）
    const s = Isles.settings();
    const wOf = (k) => Number.isFinite(Number(s[`w_${k}`])) ? Math.max(0, Number(s[`w_${k}`])) : 1;
    const pairs = ["cpu", "gpu", "mem"].map((k) => [METRICS[k].get(data), wOf(k)])
      .filter(([v]) => Number.isFinite(v));
    const wsum = pairs.reduce((a, [, w]) => a + w, 0);
    const overall = wsum > 0 ? pairs.reduce((a, [v, w]) => a + v * w, 0) / wsum : 0;
    // 每格秒数 = 设置（5–120 步进 5，非法回落 30）：攒满窗口才推进一格；
    // 窗口累计期间尾格实时预览运行均值（活格，提交后定格）
    const secs = Number.isFinite(Number(s.heat_seconds)) ? Math.max(5, Math.min(120, Number(s.heat_seconds))) : 30;
    heatAcc.push(Math.max(0, Math.min(100, overall)));
    if (heatAcc.length >= secs) {
      const mean = heatAcc.reduce((a, b) => a + b, 0) / heatAcc.length;
      heatAcc = [];
      heatBuf.push(mean);
      if (heatBuf.length > heatCells.length) heatBuf.shift();
      drawHeat();
    } else {
      drawHeat(heatAcc.reduce((a, b) => a + b, 0) / heatAcc.length);
    }
  }

  function start() {
    if (timer) return;
    poll();
    timer = setInterval(poll, 1000);
  }
  function stop() {
    clearInterval(timer);
    timer = null;
  }
  document.addEventListener("visibilitychange", () => (document.hidden ? stop() : start()));

  // ── 接线与启动 ──
  document.addEventListener("desk-setting-changed", (e) => {
    const { key, value } = e.detail || {};
    if (key === "size") applySize(value);
    if (key === "theme") applyTheme(value);
    if (SLOTS.includes(key)) onRingKeyChange(key, value);
    applyStatic();
    drawHeat();   // 换色/主题后热力图需重绘（档色含卡底混色）
    syncMenu();
  });
  document.addEventListener("desk-language-changed", () => { applyStatic(); buildHeat(); });

  // 管理器「窗口」页可开拖拽缩放：宽度变了热力图列数跟着重算（防抖 150ms）
  let _resizeTimer = 0;
  window.addEventListener("resize", () => {
    clearTimeout(_resizeTimer);
    _resizeTimer = setTimeout(buildHeat, 150);
  });

  buildRings();
  applyStatic();
  reconcileRingState().then(() => { buildHeat(); start(); syncMenu(); });
})();
