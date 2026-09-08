// 屿 · 天气（isles-weather）：当下温度/状况 + 未来五日预报。
// 数据源 Open-Meteo（预报 + 地理编码，免密钥）；权限 network（低危）。
// 图标 = Meteocons（vendor 进包，许可见 icons/LICENSE.txt）。
(() => {
  const $ = (id) => document.getElementById(id);
  const SIZES = { mini: [200, 200], wide: [420, 200] };

  // ── WMO 天气码 → 图标 + 双语名称 ──
  const WMO = {
    0: ["clear", "晴", "Clear"],
    1: ["partly", "大部晴朗", "Mostly clear"],
    2: ["partly", "多云", "Partly cloudy"],
    3: ["overcast", "阴", "Overcast"],
    45: ["fog", "雾", "Fog"], 48: ["fog", "冻雾", "Rime fog"],
    51: ["drizzle", "毛毛雨", "Drizzle"], 53: ["drizzle", "毛毛雨", "Drizzle"], 55: ["drizzle", "毛毛雨", "Drizzle"],
    56: ["drizzle", "冻毛毛雨", "Freezing drizzle"], 57: ["drizzle", "冻毛毛雨", "Freezing drizzle"],
    61: ["rain", "小雨", "Light rain"], 63: ["rain", "中雨", "Rain"], 65: ["rain", "大雨", "Heavy rain"],
    66: ["rain", "冻雨", "Freezing rain"], 67: ["rain", "冻雨", "Freezing rain"],
    71: ["snow", "小雪", "Light snow"], 73: ["snow", "中雪", "Snow"], 75: ["snow", "大雪", "Heavy snow"], 77: ["snow", "雪粒", "Snow grains"],
    80: ["rain", "阵雨", "Showers"], 81: ["rain", "阵雨", "Showers"], 82: ["rain", "强阵雨", "Violent showers"],
    85: ["snow", "阵雪", "Snow showers"], 86: ["snow", "阵雪", "Snow showers"],
    95: ["thunderstorms", "雷暴", "Thunderstorm"],
    96: ["thunderstorms-rain", "雷暴伴冰雹", "Thunderstorm w/ hail"], 99: ["thunderstorms-rain", "雷暴伴冰雹", "Thunderstorm w/ hail"],
  };
  const iconOf = (code, isDay) => {
    const base = WMO[code]?.[0] || "overcast";
    const dayNight = (base === "clear" || base === "partly") ? (isDay ? "day" : "night") : "";
    const name = base === "partly" ? `partly-cloudy-${dayNight}` : dayNight ? `${base}-${dayNight}` : base;
    return `icons/${name}.svg`;
  };
  const condText = (code) => { const w = WMO[code]; return w ? Isles.t(w[1], w[2]) : Isles.t("未知", "Unknown"); };

  // ── 尺寸档（与设置面板/右键菜单同一设置项） ──
  function applySize(size) {
    const wh = SIZES[size] || SIZES.wide;
    Isles.bridge()?.invoke?.("skin_set_window_config", { patch: { width: wh[0], height: wh[1] } }).catch(() => {});
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

  // ── 主题预设（写 bg/text 两色，单项可再微调） ──
  const THEMES = {
    // 族主题四套（浅 3 + 暗 1；青柠为基准不调）
    sora:     { bg: "#F0F6FC", text: "#17334E" },
    lime:     { bg: "#F1FAF3", text: "#1E4030" },
    lavender: { bg: "#F6F1FC", text: "#382D52" },
    nord:     { bg: "#1C2230", text: "#E7EEF7" },
  };
  async function applyTheme(name) {
    const t = THEMES[name];
    const invoke = Isles.bridge()?.invoke;
    if (!t || !invoke) return;
    await invoke("skin_set_setting", { key: "bg_color", value: t.bg }).catch(() => {});
    await invoke("skin_set_setting", { key: "text_color", value: t.text }).catch(() => {});
    // 主题色本地先行落 DOM（异步回同步竞态——见 applyStatic 注释）
    applyStatic({ bg_color: t.bg, text_color: t.text });
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
  }

  // ── 数据链：城市 → 地理编码（内存缓存）→ 预报 + 空气质量 ──
  let geoCache = null;   // { name, lat, lon } — 会话内缓存
  let refreshGen = 0;    // 代际令牌：换城市/语言时在途请求作废（旧响应后到
                         // 不得渲染/落盘——慢网下错显旧城市一整个刷新间隔的事故）

  // ── 持久缓存（weather-cache.json，自写免权限沙箱）：有效期 = 刷新间隔，
  // 有效期内零请求；跨天（过 0 点）必失效——日期变了预报必须重拉；
  // 城市不变时坐标也吃缓存（不再请求地理编码） ──
  const CACHE_FILE = "weather-cache.json";
  let memCache = null;   // { input, name, lat, lon, fetchedAt, dayKey, fc }
  const dayKeyOf = (t) => { const d = new Date(t); return `${d.getFullYear()}-${d.getMonth() + 1}-${d.getDate()}`; };

  async function loadCache() {
    if (memCache) return memCache;
    try {
      const text = await Isles.bridge()?.invoke?.("skin_read_file", { path: CACHE_FILE });
      const c = JSON.parse(text);
      if (c && c.fc && Number.isFinite(c.fetchedAt)) memCache = c;
    } catch {}
    return memCache;
  }

  function cacheValid(c, city, minMs) {
    return !!c && c.input === city
      && c.lang === Isles.lang()   // 城市显示名随语言——换语言必须重解析
      && (Date.now() - c.fetchedAt) < minMs
      && c.dayKey === dayKeyOf(Date.now());   // 跨天即失效
  }

  async function saveCache(c) {
    memCache = c;
    await Isles.bridge()?.invoke?.("skin_write_file", { path: CACHE_FILE, data: JSON.stringify(c) }).catch(() => {});
  }
  let timer = null;
  const _warned = new Set();
  function warnOnce(what, e) {
    if (_warned.has(what)) return;
    _warned.add(what);
    Isles.bridge()?.invoke?.("skin_log", { level: "warn", message: `天气 ${what} 失败（后续不再重复记录）: ${e}` })?.catch(() => {});
  }

  async function httpJson(url) {
    const res = await Isles.bridge()?.invoke?.("http_request", { url, timeoutMs: 12000 });
    if (!res || res.status < 200 || res.status >= 300) throw new Error(`HTTP ${res?.status ?? "?"}`);
    return JSON.parse(res.body);
  }

  async function resolveCity() {
    const city = (Isles.settings().city || "").trim();
    if (geoCache && geoCache.input === city && geoCache.lang === Isles.lang()) return geoCache;
    if (!city) return null;
    // 地理编码语言跟随界面——英文界面城市名不再落中文（审查建议）
    const data = await httpJson(`https://geocoding-api.open-meteo.com/v1/search?name=${encodeURIComponent(city)}&count=1&language=${Isles.lang() === "en" ? "en" : "zh"}&format=json`);
    const hit = data?.results?.[0];
    if (!hit) throw new Error(Isles.t(`找不到城市「${city}」`, `city "${city}" not found`));
    geoCache = { input: city, lang: Isles.lang(), name: hit.name || city, lat: hit.latitude, lon: hit.longitude };
    return geoCache;
  }

  async function refresh() {
    const invoke = Isles.bridge()?.invoke;
    if (!invoke) return;
    const gen = ++refreshGen;   // 本次刷新领取代际——任何在途旧刷新的响应作废
    const s = Isles.settings();
    const city = (s.city || "").trim();
    if (!city) { renderHint(); return; }

    // 缓存有效（同城市 + 同语言 + 未过刷新间隔 + 未跨天）→ 直接用缓存渲染，零请求
    const mins = Number(s.refresh_min);
    const minMs = (Number.isFinite(mins) ? Math.max(10, Math.min(120, mins)) : 30) * 60000;
    const cached = await loadCache();
    if (gen !== refreshGen) return;
    if (cacheValid(cached, city, minMs)) {
      render({ name: cached.name }, cached.fc);
      return;
    }
    renderLoading();   // 有城市无有效缓存 → 整卡加载态，数据到了才上内容

    // 坐标：同城市同语言吃缓存/会话缓存，否则地理编码
    let geo = null;
    if (cached?.input === city && cached.lang === Isles.lang()) geo = { input: city, name: cached.name, lat: cached.lat, lon: cached.lon };
    let geoNotFound = false;
    if (!geo) {
      try { geo = await resolveCity(); } catch (e) {
        geoNotFound = /not found|找不到/.test(String(e && e.message));
        warnOnce("地理编码", e);
      }
      if (gen !== refreshGen) return;
      if (!geo) {
        if (cached) { render({ name: cached.name }, cached.fc); return; }   // 失败回落旧缓存帧
        // 找不到城市（拼错 = 永久语义）与网络抖动（下个节拍重试）分流——
        // 拼错的城市不该永远停在「正在加载」（审查应修）
        if (geoNotFound) renderNotFound(city);
        return;
      }
    }

    let fc = null;
    try {
      fc = await httpJson(`https://api.open-meteo.com/v1/forecast?latitude=${geo.lat}&longitude=${geo.lon}&current=temperature_2m,weather_code,is_day&daily=weather_code,temperature_2m_max,temperature_2m_min&forecast_days=6&timezone=auto`);
    } catch (e) { warnOnce("预报", e); }
    if (gen !== refreshGen) return;
    if (!fc) {
      if (cached) render({ name: cached.name }, cached.fc);   // 失败回落旧缓存帧（跨天也照显，比白屏好）
      return;
    }
    render({ name: geo.name }, fc);
    saveCache({ input: city, lang: Isles.lang(), name: geo.name, lat: geo.lat, lon: geo.lon, fetchedAt: Date.now(), dayKey: dayKeyOf(Date.now()), fc });
  }

  function renderHint() {
    // 未设城市：整卡空态引导（不是左区一行小字）
    $("card").classList.remove("loading");
    $("card").classList.add("no-city");
    $("empty-text").textContent = Isles.t("在设置中填写城市名", "Set a city in settings");
  }

  function renderLoading() {
    // 城市已设但数据未达：整卡加载提示——绝不露出 --° + 破图标的半成品帧
    //（实机反馈）；取数失败且无缓存可回落时保持此态，下个刷新节拍再试
    $("card").classList.remove("no-city");
    $("card").classList.add("loading");
    $("empty-text").textContent = Isles.t("正在加载天气数据", "Loading weather data");
  }

  function renderNotFound(city) {
    // 找不到城市（拼错 = 永久语义）：整卡明示，不再永远停在加载态
    $("card").classList.remove("loading");
    $("card").classList.add("no-city");
    $("empty-text").textContent = Isles.t(`找不到城市「${city}」`, `City "${city}" not found`);
  }

  const WEEK_ZH = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
  const WEEK_EN = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

  function render(geo, fc) {
    $("card").classList.remove("no-city", "loading");
    const cur = fc.current || {};
    const daily = fc.daily || {};
    const code = cur.weather_code;
    const isDay = cur.is_day !== 0;

    $("temp").textContent = `${Math.round(cur.temperature_2m ?? 0)}°`;
    $("cond-icon").src = iconOf(code, isDay);
    $("cond").textContent = condText(code);
    // 低温在前（低到高）——用户直觉与通行天气应用一致（高~低曾实机反馈反直觉）
    const hi = Math.round(daily.temperature_2m_max?.[0] ?? 0);
    const lo = Math.round(daily.temperature_2m_min?.[0] ?? 0);
    $("meta").textContent = `${lo}°~${hi}° · ${geo.name}`;

    // 未来五日预报（跳过今天——左区已是当下，不重复）；列内 = 星期/图标/温度区间/日期
    const box = $("forecast");
    box.textContent = "";
    const times = daily.time || [];
    for (let i = 1; i <= 5 && i < times.length; i++) {
      const date = new Date(`${times[i]}T12:00:00`);
      const d = document.createElement("div");
      d.className = "day";
      const dw = document.createElement("span");
      dw.className = "dw";
      dw.textContent = i === 1 ? Isles.t("明天", "Tmrw") : Isles.t(WEEK_ZH[date.getDay()], WEEK_EN[date.getDay()]);
      const img = document.createElement("img");
      img.src = iconOf(daily.weather_code?.[i], true);
      img.alt = "";
      const dt = document.createElement("span");
      dt.className = "dtemp";
      dt.textContent = `${Math.round(daily.temperature_2m_min?.[i] ?? 0)}°~${Math.round(daily.temperature_2m_max?.[i] ?? 0)}°`;
      const dd = document.createElement("span");
      dd.className = "dt";
      dd.textContent = `${date.getDate()}${Isles.t("日", "")}`;
      d.appendChild(dw);
      d.appendChild(img);
      d.appendChild(dt);
      d.appendChild(dd);
      box.appendChild(d);
    }
  }

  // ── 定时刷新（页面隐藏暂停，恢复立即补一拍） ──
  function restart() {
    stop();
    const mins = Number(Isles.settings().refresh_min);
    const ms = (Number.isFinite(mins) ? Math.max(10, Math.min(120, mins)) : 30) * 60000;
    refresh();
    timer = setInterval(refresh, ms);
  }
  function stop() { clearInterval(timer); timer = null; }
  document.addEventListener("visibilitychange", () => (document.hidden ? stop() : restart()));

  // ── 接线与启动 ──
  document.addEventListener("desk-setting-changed", (e) => {
    const { key, value } = e.detail || {};
    if (key === "size") applySize(value);
    if (key === "theme") applyTheme(value);
    if (key === "city") { geoCache = null; memCache = null; refresh(); }
    if (key === "refresh_min") restart();
    applyStatic();
    syncMenu();
  });
  document.addEventListener("desk-language-changed", () => { applyStatic(); geoCache = null; refresh(); });

  applyStatic();
  // 首帧即整卡态：有城市 → 加载中；无城市 → 引导（绝不让 --° + 破图标闪出）
  if ((Isles.settings().city || "").trim()) renderLoading(); else renderHint();
  restart();
  syncMenu();
  Isles.onMidnight(refresh);   // 跨零点即重拉（dayKey 失效不等下个刷新节拍）
})();
