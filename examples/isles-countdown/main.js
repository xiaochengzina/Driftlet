// 屿 · 倒数日：整天差主指标。零权限，跨零点自动重算。
// 纸卡（bg/text/accent 三件套 + 主题预设，与族同款）；自定义图片为可选
// photo 族（白族文字 + 暗纱 + 模糊）。场景颜色预设已随族视觉统一轮移除。
//
// 尺寸：设置「尺寸」预设两档（横条 420×200 / 小方 200×200）→ 对自己
// skin_set_window_config 免权限自 resize；布局经容器查询随窗切换
//（≤240px 宽 = M 小方布局，其余 = P 横条布局）。加载时不回写尺寸——尊重
// 用户在管理器的手动调整。
//
// 自定义背景：页内 <input type=file> 选图 → canvas 降采样（≤1600px JPEG）
// → skin_write_file 存皮肤目录（免权限，≤16MB）→ CSS 变量注入。皮肤更新
// 会重建目录、背景图随之丢失（一键重选即可）——settings 值上限 4000 字符
// 存不下图片，这是零权限下唯一的持久化通道。
(() => {
  const $ = (id) => document.getElementById(id);
  const SIZES = { mini: [200, 200], wide: [420, 200] };
  const BG_FILE = "user-bg.jpg";
  let customBg = false;
  let customUrl = null;
  const bgEl = document.querySelector(".isles-bg");
  let firstPaint = true;    // 加载首帧不做切换过场

  // 底图切换过场：短暂压低背景层再回升（base.css 的 opacity 过渡）
  function pulseBg() {
    bgEl.style.opacity = "0.35";
    requestAnimationFrame(() => requestAnimationFrame(() => { bgEl.style.opacity = ""; }));
  }

  // ── 尺寸档（设置「尺寸」选档 → 对自己 skin_set_window_config 免权限自 resize；
  // 未知/遗留值兜底默认档小方；加载时不回写——尊重用户在管理器的手动调整） ──
  function applySize(size) {
    const wh = SIZES[size] || SIZES.mini;
    const invoke = Isles.bridge()?.invoke;
    if (!invoke) return;
    invoke("skin_set_window_config", { patch: { width: wh[0], height: wh[1] } }).catch(() => {});
  }

  // ── 右键菜单尺寸档（与设置面板同一设置项，勾选态 = 当前档） ──
  function currentSize() {
    const v = Isles.settings().size;
    return SIZES[v] ? v : "mini";
  }

  function syncMenu(current) {
    const cur = current || currentSize();
    Isles.setMenuItems([
      { id: "mini", label_zh: "小方 1×1", label_en: "Mini 1×1", checked: cur === "mini" },
      { id: "wide", label_zh: "横条 2×1", label_en: "Wide 2×1", checked: cur === "wide" },
    ]);
  }

  Isles.onMenuItem((id) => {
    if (!SIZES[id]) return;
    // 与设置面板同口径：写回设置项（皮肤侧写入不会回派 desk-setting-changed，
    // 但桥内 settings 会静默同步）+ 立即 resize + 菜单勾选翻转
    Isles.bridge()?.invoke?.("skin_set_setting", { key: "size", value: id }).catch(() => {});
    applySize(id);
    syncMenu(id);
  });

  // ── 静态外观（主题/纸卡/图片/accent/文案） ───────────────────────────
  // 纸卡（默认）= 族统一浅色纸卡（bg/text/accent 三个颜色项 + 主题预设）；
  // 自定义图片 = photo 族（白族文字 + 暗纱 + 模糊）。
  const THEMES = {
    // 族主题四套（浅 3 + 暗 1；青柠为基准不调）
    sora:     { bg: "#F0F6FC", text: "#17334E", accent: "#2C82E4" },
    lime:     { bg: "#F1FAF3", text: "#1E4030", accent: "#17B978" },
    lavender: { bg: "#F6F1FC", text: "#382D52", accent: "#7D5BE7" },
    nord:     { bg: "#1C2230", text: "#E7EEF7", accent: "#7FDCC9" },
  };
  const THEME_KEYS = { bg: "bg_color", text: "text_color", accent: "accent" };
  const HEX = /^#([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;
  const setVar = (el, name, hex) => {
    if (typeof hex === "string" && HEX.test(hex)) el.style.setProperty(name, hex);
  };

  async function applyTheme(name) {
    const invoke = Isles.bridge()?.invoke;
    if (!invoke) return;
    // 「图片背景」预设：白字 + 黑暗纱（配自定义背景图用）——不动 bg/accent
    if (name === "photo") {
      await invoke("skin_set_setting", { key: "text_color", value: "#FFFFFF" }).catch(() => {});
      await invoke("skin_set_setting", { key: "scrim_color", value: "#000000" }).catch(() => {});
      applyStatic({ text_color: "#FFFFFF", scrim_color: "#000000" });
      return;
    }
    const t = THEMES[name];
    if (!t) return;
    for (const [prop, hex] of Object.entries(t)) {
      await invoke("skin_set_setting", { key: THEME_KEYS[prop], value: hex }).catch(() => {});
    }
    // 主题色本地先行落 DOM（异步回同步竞态——见 applyStatic 注释）
    const over = {};
    for (const [prop, hex] of Object.entries(t)) over[THEME_KEYS[prop]] = hex;
    applyStatic(over);
  }

  function applyStatic(over) {
    // over = 本地刚写入的主题色覆盖——桥内 settings 的静默同步是异步 eval，
    // 此刻回读会拿旧值（六皮同款竞态，审查应修）；事件流随后对齐
    const s = over ? { ...Isles.settings(), ...over } : Isles.settings();
    Isles.setAccent(s.accent);

    const card = $("card");
    card.classList.toggle("isles-card--photo", customBg);
    if (customBg) {
      // 自定义图片卡：白族文字 + 定向暗纱 + 背景模糊（族 photo 机制）
      card.style.removeProperty("--isles-card-bg");
      card.style.removeProperty("--isles-fg");
      Isles.setTextColor(card, s.text_color);
      Isles.setScrimColor(card, s.scrim_color);
      Isles.setBgBlur(card, s.bg_blur);
    } else {
      // 纸卡：bg/text/accent 三件套（族同款）；图片态的覆盖全部拆除
      setVar(card, "--isles-card-bg", s.bg_color);
      // 「图片背景」主题写了白字但没选图 → 回落安全深色（全卡白字白底不可读
      // 的失败态，审查建议）；选了图走上面的 photo 分支，白字正常生效
      const textHex = (s.theme === "photo" && !customBg) ? "#17334E" : s.text_color;
      setVar(card, "--isles-fg", textHex);
      card.style.removeProperty("--isles-on-photo");
      card.style.removeProperty("--isles-scrim-color");
      card.style.removeProperty("--isles-bg-blur-filter");
    }

    // display 真移除（visibility 占位隐藏会把小方档的主指标顶偏）
    $("date").style.display = s.show_date === false ? "none" : "";
    // 单按钮双态：图标由 CSS 按 isles-card--custom 切换，这里只更新提示文案
    const bgLabel = customBg
      ? Isles.t("恢复默认卡面", "Back to default card")
      : Isles.t("自定义背景图", "Custom background");
    $("btn-bg").title = bgLabel;
    $("btn-bg").setAttribute("aria-label", bgLabel);
    firstPaint = false;
  }

  // ── 主指标（日期任务列表：自动倒数最近的一个，当天停留一天，过完跳下一个） ──
  function pickTask(tasks) {
    const now = new Date();
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    let upcoming = null, latestPast = null;
    for (const t of Array.isArray(tasks) ? tasks : []) {
      const d = Isles.parseDay(t && t.time);   // time = "YYYY-MM-DD HH:MM:SS"，取日期前缀
      if (!d) continue;                        // time 可空的条目无法倒数，跳过
      const diff = Math.round((d - today) / 86400000);
      // 加载路径对 datetasklist 只查 is_array 不查条目形状——手编
      // settings.json 注入非字符串 text 时 .trim() 会炸掉整皮；String() 兜底
      const entry = { diff, text: String((t && t.text) ?? "").trim(), date: d };
      if (diff >= 0 && (!upcoming || diff < upcoming.diff)) upcoming = entry;
      else if (diff < 0 && (!latestPast || diff > latestPast.diff)) latestPast = entry;
    }
    return upcoming || latestPast;   // 全部已过 → 显示最近一个的已过天数
  }

  function render() {
    const task = pickTask(Isles.settings().tasks);

    if (!task) {
      $("title").textContent = Isles.t("倒数日", "Countdown");
      $("state").textContent = Isles.t("请在设置中添加倒数日", "Add countdowns in settings");
      $("days").textContent = "--";
      $("unit").textContent = "";
      $("date").textContent = "";
      return;
    }

    const name = task.text || Isles.t("倒数日", "Countdown");
    const { diff, date } = task;

    // 标题 + 副题双行（zh/en 同构）：名称 / 还有·已过；当天副题留空
    $("title").textContent = name;
    $("state").textContent = diff > 0 ? Isles.t("还有", "to go") : diff === 0 ? "" : Isles.t("已过", "passed");

    $("unit").textContent = diff === 0 ? "" : Isles.t("天", Math.abs(diff) === 1 ? "day" : "days");
    $("days").textContent = diff === 0 ? Isles.t("今天", "Today") : Math.abs(diff);
    $("days").classList.toggle("days-passed", diff < 0);
    $("days").classList.toggle("days-word", diff === 0);   // 文字态降字级（见样式）

    $("date").textContent = `${date.getFullYear()}${Isles.t("年", "-")}${Isles.pad2(date.getMonth() + 1)}${Isles.t("月", "-")}${Isles.pad2(date.getDate())}${Isles.t("日", "")}`;
  }

  // ── 自定义背景图 ──────────────────────────────────────────
  function setCustomBg(on, url, animate = false) {
    const changed = on !== customBg || (on && url !== customUrl);
    customBg = on;
    customUrl = on ? url : null;
    const card = $("card");
    card.classList.toggle("isles-card--custom", on);
    if (on) card.style.setProperty("--isles-custom-bg", `url("${url}")`);
    else card.style.removeProperty("--isles-custom-bg");
    if (animate && changed && !firstPaint) pulseBg();
    applyStatic();   // 按钮提示文案随态翻转
  }

  // 加载时探测目录里是否已有 user-bg.jpg（有 → 用图，无 → 纸卡）
  function probeCustomBg() {
    const url = `${BG_FILE}?v=${Date.now()}`;
    const img = new Image();
    img.onload = () => setCustomBg(true, url);
    img.onerror = () => setCustomBg(false);
    img.src = url;
  }

  function downscale(file, maxEdge, quality) {
    return new Promise((resolve, reject) => {
      const url = URL.createObjectURL(file);
      const img = new Image();
      img.onload = () => {
        URL.revokeObjectURL(url);
        const scale = Math.min(1, maxEdge / Math.max(img.width, img.height));
        const w = Math.max(1, Math.round(img.width * scale));
        const h = Math.max(1, Math.round(img.height * scale));
        const c = document.createElement("canvas");
        c.width = w;
        c.height = h;
        const ctx = c.getContext("2d");
        // JPEG 无透明通道：透明 PNG 直接绘制透明处会变黑，先垫底纯白
        ctx.fillStyle = "#FFFFFF";
        ctx.fillRect(0, 0, w, h);
        ctx.drawImage(img, 0, 0, w, h);
        resolve(c.toDataURL("image/jpeg", quality));
      };
      img.onerror = () => {
        URL.revokeObjectURL(url);
        reject(new Error("image decode failed"));
      };
      img.src = url;
    });
  }

  async function onPicked(e) {
    const f = e.target.files && e.target.files[0];
    e.target.value = "";
    if (!f) return;
    try {
      const dataUrl = await downscale(f, 1600, 0.85);
      const invoke = Isles.bridge()?.invoke;
      if (!invoke) {
        // 无桥（浏览器裸开调试）：本会话生效，不落盘
        setCustomBg(true, dataUrl, true);
        return;
      }
      await invoke("skin_write_file", {
        path: BG_FILE,
        data: dataUrl.slice(dataUrl.indexOf(",") + 1),
        binary: true,
      });
      setCustomBg(true, `${BG_FILE}?v=${Date.now()}`, true);
    } catch (err) {
      Isles.bridge()?.invoke?.("skin_log", { level: "warn", message: `自定义背景保存失败: ${err}` })?.catch(() => {});
    }
  }

  async function clearBg() {
    try {
      await Isles.bridge()?.invoke?.("skin_delete_file", { path: BG_FILE });
      setCustomBg(false, null, true);
    } catch (err) {
      // 删除失败就不切回纸卡——文件还在盘上，下回加载会复活（审查建议：
      // 静默切回的行为与用户预期相反）；记一条 warn 供日志窗口排查
      Isles.bridge()?.invoke?.("skin_log", { level: "warn", message: `自定义背景删除失败: ${err}` })?.catch(() => {});
    }
  }

  // ── 接线 ─────────────────────────────────────────────────
  // 单按钮双态：有自定义背景 → 点击恢复渐变；无 → 点击选图
  $("btn-bg").addEventListener("click", () => (customBg ? clearBg() : $("file").click()));
  $("file").addEventListener("change", onPicked);

  applyStatic();
  render();
  probeCustomBg();
  syncMenu();   // 注册右键菜单尺寸档
  Isles.onMidnight(render);

  document.addEventListener("desk-setting-changed", (e) => {
    const { key, value } = e.detail || {};
    if (key === "size") applySize(value);
    if (key === "theme") applyTheme(value);
    applyStatic();
    render();
    syncMenu();   // 管理器侧改尺寸也要翻转菜单勾选
  });
  document.addEventListener("desk-language-changed", () => { applyStatic(); render(); });
})();
