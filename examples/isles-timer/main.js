// 屿 · 计时器（isles-timer）：倒计时 + 正向计时，M 档 200×200 单形态。
// 权限 notify（低危）：到时弹 Windows 通知中心通知（可在设置关闭）。
// 圆环与 isles-monitor 同款（r50 / 线宽 6 / 圆头 / -90° 起笔 / 轨道 = accent
// 16% 混入卡底）：倒计时环随剩余时间消退，正向计时环每分钟扫一圈。
// 环内步进器：倒计时·就绪态悬停卡面浮现 ±（上下各一），点按 ±1 分钟、
// 按住连发——与设置面板「倒计时时长」是同一设置项（皮肤写入不回派事件，
// 本地直接应用）。
// 计时内核走墙钟（Date.now）——页面隐藏/系统休眠唤醒自动校准，无累积漂移；
// 状态持久化 timer-state.json（skin_write_file 自写皮肤目录，免权限）——
// 重载/重启续跑：倒计时存绝对终点 endAt，正向计时存累计值 + 保存时刻。
// 到时提示音 = WebAudio 上行琶音（零素材离线可用；AudioContext 在用户首次
// 点「开始」时解锁，无手势场景静默跳过）；「时间到」是瞬时提醒态、不跨进程
// 恢复——重开即满环就绪，提示音/通知只在真实到点那一刻的 finish() 发一次。
(() => {
  const $ = (id) => document.getElementById(id);
  const STATE_FILE = "timer-state.json";
  const R = 50;                       // 环半径（viewBox 120，与 isles-monitor 外环同值）
  const CIRC = 2 * Math.PI * R;
  const SAVE_INTERVAL = 15000;        // 运行中周期性落盘间隔（崩溃/重启最多回退 15s）
  const UP_CAP = 99 * 3600 * 1000;    // 正向计时显示上限 99h（离家忘关不会涨成天数）
  const MIN_MIN = 1, MAX_MIN = 180;   // 倒计时时长界（与 skin.json 的 min/max 一致）

  // ── 设置读取（防御：非法值回落默认） ──
  function currentMode() {
    return Isles.settings().mode === "up" ? "up" : "down";
  }
  function currentMinutes() {
    const n = Number(Isles.settings().minutes);
    return Number.isFinite(n) ? Math.max(MIN_MIN, Math.min(MAX_MIN, Math.round(n))) : 25;
  }
  function totalMs() {
    return currentMinutes() * 60000;
  }

  // ── 计时状态 ──
  // { mode, phase: idle|running|paused|done, totalMs, remainMs, elapsedMs,
  //   endAt（down running 绝对终点）, upStart（up running 起点 = now − 已累计） }
  function freshState(mode) {
    const t = totalMs();
    return { mode, phase: "idle", totalMs: t, remainMs: t, elapsedMs: 0, endAt: 0, upStart: 0 };
  }
  let st = freshState(currentMode());
  let lastSave = 0;

  function snapshot() {
    const now = Date.now();
    const d = { v: 1, mode: st.mode, phase: st.phase, savedAt: now };
    if (st.mode === "down") {
      d.totalMs = st.totalMs;
      if (st.phase === "running") d.endAt = st.endAt;
      else if (st.phase === "paused") d.remainMs = st.remainMs;
    } else {
      if (st.phase === "running") d.elapsedMs = now - st.upStart;
      else if (st.phase === "paused") d.elapsedMs = st.elapsedMs;
    }
    return d;
  }

  function save() {
    lastSave = Date.now();
    Isles.bridge()?.invoke?.("skin_write_file", { path: STATE_FILE, data: JSON.stringify(snapshot()) })?.catch(() => {});
  }

  // 恢复口径：模式以设置为准（存档模式不符 → 弃档新起）；「时间到」是瞬时
  // 提醒态、不跨进程——关闭前已完成未重置的、离开期间过期的，重开一律满环
  // 就绪（实机反馈：重启后看到隔夜的「时间到」毫无意义，还得手动重置）
  async function load() {
    let d = null;
    try {
      const text = await Isles.bridge()?.invoke?.("skin_read_file", { path: STATE_FILE });
      d = JSON.parse(text);
    } catch { return; }
    if (!d || d.v !== 1 || d.mode !== currentMode()) return;
    const now = Date.now();
    if (d.mode === "down") {
      const tot = Number.isFinite(d.totalMs) && d.totalMs >= 60000 && d.totalMs <= MAX_MIN * 60000 ? d.totalMs : totalMs();
      if (d.phase === "running" && Number.isFinite(d.endAt) && d.endAt > now) {
        st = { ...freshState("down"), totalMs: tot, endAt: d.endAt, phase: "running" };
      } else if (d.phase === "paused" && Number.isFinite(d.remainMs)) {
        st = { ...freshState("down"), totalMs: tot, phase: "paused", remainMs: Math.max(0, Math.min(tot, d.remainMs)) };
      }
      // done / 离开期间过期 → 不恢复，保持默认 idle（满环就绪）
    } else {
      if (d.phase === "running" && Number.isFinite(d.elapsedMs) && Number.isFinite(d.savedAt)) {
        const el = Math.min(UP_CAP, Math.max(0, d.elapsedMs + (now - d.savedAt)));
        st = { ...freshState("up"), phase: "running", elapsedMs: el, upStart: now - el };
      } else if (d.phase === "paused" && Number.isFinite(d.elapsedMs)) {
        st = { ...freshState("up"), phase: "paused", elapsedMs: Math.min(UP_CAP, Math.max(0, d.elapsedMs)) };
      }
    }
  }

  // ── 到时提示音（WebAudio 上行琶音 G5→B5→E6；需用户手势解锁——
  //    「开始」按钮点击即解锁；未解锁/无桥场景静默跳过） ──
  let actx = null;
  function ensureAudio() {
    try {
      const AC = window.AudioContext || window.webkitAudioContext;
      if (!AC) return;
      if (!actx) actx = new AC();
      if (actx.state === "suspended") actx.resume().catch(() => {});
    } catch { actx = null; }
  }
  function beep() {
    if (Isles.settings().sound === false) return;
    ensureAudio();
    if (!actx || actx.state !== "running") return;
    try {
      const t0 = actx.currentTime + 0.05;
      [784, 988, 1319].forEach((freq, i) => {
        const o = actx.createOscillator();
        const g = actx.createGain();
        o.type = "sine";
        o.frequency.value = freq;
        const t = t0 + i * 0.22;
        g.gain.setValueAtTime(0.0001, t);
        g.gain.exponentialRampToValueAtTime(0.15, t + 0.02);
        g.gain.exponentialRampToValueAtTime(0.0001, t + 0.3);
        o.connect(g);
        g.connect(actx.destination);
        o.start(t);
        o.stop(t + 0.32);
      });
    } catch {}
  }

  // ── 到时系统通知（权限 notify 低危；隐藏/最小化也能收到；设置可关） ──
  function notifyDone() {
    if (Isles.settings().notify === false) return;
    const mins = Math.round(st.totalMs / 60000);
    Isles.bridge()?.invoke?.("show_notification", {
      title: Isles.t("倒计时时间到", "Time's up"),
      body: Isles.t(`${mins} 分钟专注结束，休息一下吧`, `${mins} min of focus done — take a break`),
    })?.catch((e) => {
      Isles.bridge()?.invoke?.("skin_log", { level: "warn", message: `到时系统通知失败: ${e}` })?.catch(() => {});
    });
  }

  // ── 状态迁移 ──
  function start() {
    ensureAudio();
    const now = Date.now();
    if (st.mode === "down") {
      if (st.phase === "done" || st.phase === "idle") {
        st.totalMs = totalMs();          // 新一趟吃最新时长设置
        st.remainMs = st.totalMs;
      }
      st.endAt = now + st.remainMs;
    } else {
      if (st.phase !== "paused") st.elapsedMs = 0;
      st.upStart = now - st.elapsedMs;
    }
    st.phase = "running";
    save(); render(); syncMenu(); tick();
  }

  function pause() {
    if (st.phase !== "running") return;
    const now = Date.now();
    if (st.mode === "down") st.remainMs = Math.max(0, st.endAt - now);
    else st.elapsedMs = now - st.upStart;
    st.phase = "paused";
    save(); render(); syncMenu(); tick();
  }

  function reset() {
    st = freshState(st.mode);
    save(); render(); syncMenu(); tick();
  }

  function finish() {
    st.phase = "done";
    st.remainMs = 0;
    save(); render(); syncMenu();
    tick();   // 节拍已随 render 内的 syncTick 收起——到点帧要手动补一拍（满环 + 00:00）
    beep();
    notifyDone();
  }

  function toggle() {
    if (st.phase === "running") pause();
    else start();
  }

  function setMode(m) {
    if ((m !== "down" && m !== "up") || m === st.mode) return;
    st = freshState(m);
    save(); render(); syncMenu(); tick();
  }

  // 时长设置改动：空闲/已结束 → 立即按新时长回到就绪；进行中不动（下次开始生效）
  function onMinutesChange() {
    if (st.phase === "idle" || st.phase === "done") {
      st = freshState(st.mode);
      save();
    }
  }

  // ── 环内步进器（倒计时·就绪态专属）：点按 ±1 分钟、按住 450ms 后 90ms 连发；
  // 写「倒计时时长」设置项（管理器面板同步）。
  // 基准值取本地 st.totalMs（= 卡面当前值）而非回读设置——skin_set_setting
  // 对桥内 settings 的静默同步走异步 eval，写完立刻回读拿到的是旧值
  //（点一下卡面不动的实机 bug）；idle 态 st 与设置恒一致（freshState 同步） ──
  function stepMinutes(dir) {
    if (st.mode !== "down" || st.phase !== "idle") return;
    const cur = Math.round(st.totalMs / 60000);
    const next = Math.max(MIN_MIN, Math.min(MAX_MIN, cur + dir));
    if (next === cur) return;
    Isles.bridge()?.invoke?.("skin_set_setting", { key: "minutes", value: next })?.catch(() => {});
    st.totalMs = st.remainMs = next * 60000;   // 本地立即生效，不等设置回同步
    save(); render(); tick();
  }

  function bindStepper(btn, dir) {
    let hold = null, repeat = null;
    const stop = () => { clearTimeout(hold); clearInterval(repeat); hold = repeat = null; };
    btn.addEventListener("pointerdown", () => {
      if (btn.disabled) return;
      stop();   // 先收旧定时器——触屏多点二次按下会泄漏一个 90ms interval 空转
      stepMinutes(dir);
      hold = setTimeout(() => { repeat = setInterval(() => stepMinutes(dir), 90); }, 450);
    });
    ["pointerup", "pointerleave", "pointercancel"].forEach((t) => btn.addEventListener(t, stop));
  }

  // ── 渲染 ──
  function fmt(sec) {
    sec = Math.max(0, Math.floor(sec));
    const h = Math.floor(sec / 3600);
    const m = Math.floor(sec / 60) % 60;
    const s = sec % 60;
    return h > 0 ? `${h}:${Isles.pad2(m)}:${Isles.pad2(s)}` : `${Isles.pad2(m)}:${Isles.pad2(s)}`;
  }

  // 环：frac = 填充比例（倒计时 = 剩余比例、从满环消退；正向计时 = 分钟相位、
  // 每分钟扫满一圈回绕）。回绕瞬切不播回卷动画（0.6s ease 会倒抹一整圈）。
  let lastOffset = CIRC;
  function setRing(frac) {
    const f = Math.max(0, Math.min(1, frac));
    const offset = CIRC * (1 - f);
    const el = $("ring-val");
    if (Math.abs(offset - lastOffset) > CIRC / 2 && st.mode === "up" && st.phase === "running") {
      el.style.transition = "none";
      el.style.strokeDashoffset = `${offset}`;
      void el.getBoundingClientRect();
      el.style.transition = "";
    } else {
      el.style.strokeDashoffset = `${offset}`;
    }
    lastOffset = offset;
  }

  let lastText = "";
  function tick() {
    const now = Date.now();
    if (st.mode === "down" && st.phase === "running" && now >= st.endAt) finish();

    let frac, text;
    if (st.mode === "down") {
      const remain = st.phase === "running" ? Math.max(0, st.endAt - now)
        : st.phase === "paused" ? st.remainMs
        : st.phase === "done" ? 0 : st.totalMs;
      // done = 环回满（完成的圆 = 完成态；弧长为零时脉动动画不可见的坑）
      frac = st.phase === "done" ? 1 : st.totalMs > 0 ? remain / st.totalMs : 0;
      text = fmt(Math.ceil(remain / 1000));
    } else {
      const elapsed = st.phase === "running" ? now - st.upStart
        : st.phase === "paused" ? st.elapsedMs : 0;
      frac = st.phase === "idle" ? 0 : (elapsed % 60000) / 60000;
      text = fmt(Math.floor(Math.min(UP_CAP, elapsed) / 1000));
    }

    setRing(frac);
    if (text !== lastText) {
      lastText = text;
      const mtime = $("mtime");
      mtime.textContent = text;
      mtime.classList.toggle("long", text.length > 5);   // H:MM:SS 小时态降字级
    }

  // 运行中周期性落盘（崩溃/重启最多回退 SAVE_INTERVAL）
    if (st.phase === "running" && now - lastSave > SAVE_INTERVAL) save();
  }

  // ── 节拍治理：200ms tick 只在「运行中 + 可见」存活——空闲/暂停/到点都是静态
  // 帧（脉动是 CSS 动画），隐藏时零轮询。例外：隐藏 + 倒计时进行中挂一个到点
  // 一次性 timeout——系统通知/提示音必须在真实到点那一刻发，不能等唤回 ──
  let tickTimer = null;
  let hideTimer = null;
  function syncTick() {
    const running = st.phase === "running";
    if (running && !document.hidden && !tickTimer) tickTimer = setInterval(tick, 200);
    if ((!running || document.hidden) && tickTimer) { clearInterval(tickTimer); tickTimer = null; }
    clearTimeout(hideTimer); hideTimer = null;
    if (running && document.hidden && st.mode === "down") {
      hideTimer = setTimeout(() => { finish(); save(); }, Math.max(0, st.endAt - Date.now()));
    }
  }

  function render() {
    const card = $("card");
    card.classList.toggle("running", st.phase === "running");
    card.classList.toggle("paused", st.phase === "paused");
    card.classList.toggle("done", st.phase === "done");
    card.classList.toggle("idle", st.phase === "idle");
    card.classList.toggle("mode-down", st.mode === "down");
    card.classList.toggle("mode-up", st.mode === "up");

    // 就绪态无文案（悬停浮现的 ± 步进器已说明一切）
    $("mstate").textContent = st.phase === "idle" ? ""
      : st.phase === "running" ? (st.mode === "down" ? Isles.t("专注中", "Focusing") : Isles.t("计时中", "Running"))
      : st.phase === "paused" ? Isles.t("已暂停", "Paused")
      : Isles.t("时间到", "Time's up");

    // 步进器到界禁用（以本地卡面值为准——设置回同步是异步的；非就绪态
    // display:none 不参与布局，按钮状态无所谓）
    const cur = Math.round(st.totalMs / 60000);
    $("btn-up").disabled = cur >= MAX_MIN;
    $("btn-down").disabled = cur <= MIN_MIN;

    const toggleLabel = st.phase === "running" ? Isles.t("暂停", "Pause")
      : st.phase === "done" ? Isles.t("重新开始", "Restart")
      : st.phase === "paused" ? Isles.t("继续", "Resume") : Isles.t("开始", "Start");
    $("btn-toggle").title = toggleLabel;
    $("btn-toggle").setAttribute("aria-label", toggleLabel);
    const resetLabel = Isles.t("重置", "Reset");
    $("btn-reset").title = resetLabel;
    $("btn-reset").setAttribute("aria-label", resetLabel);
    $("btn-up").title = Isles.t("加一分钟", "One minute more");
    $("btn-up").setAttribute("aria-label", $("btn-up").title);
    $("btn-down").title = Isles.t("减一分钟", "One minute less");
    $("btn-down").setAttribute("aria-label", $("btn-down").title);
    syncTick();   // 每次相位变迁顺手对齐节拍（render 是所有变迁的必经路）
  }

  // ── 外观（主题预设写 bg/text/环色三项，单项可再微调；自定义 = 不动） ──
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

  // ── 右键菜单：开始/暂停 + 重置 + 模式直切（勾选 = 当前态） ──
  function syncMenu() {
    const mode = st.mode;
    Isles.setMenuItems([
      { id: "toggle", label_zh: st.phase === "running" ? "暂停" : st.phase === "done" ? "重新开始" : st.phase === "paused" ? "继续" : "开始",
        label_en: st.phase === "running" ? "Pause" : st.phase === "done" ? "Restart" : st.phase === "paused" ? "Resume" : "Start" },
      { id: "reset", label_zh: "重置", label_en: "Reset" },
      { id: "down", label_zh: "倒计时", label_en: "Countdown", checked: mode === "down" },
      { id: "up", label_zh: "正向计时", label_en: "Count up", checked: mode === "up" },
    ]);
  }

  Isles.onMenuItem((id) => {
    if (id === "toggle") { toggle(); return; }
    if (id === "reset") { reset(); return; }
    if (id === "down" || id === "up") {
      Isles.bridge()?.invoke?.("skin_set_setting", { key: "mode", value: id })?.catch(() => {});
      setMode(id);
    }
  });

  // ── 接线与启动 ──
  $("btn-toggle").addEventListener("click", toggle);
  $("btn-reset").addEventListener("click", reset);
  bindStepper($("btn-up"), 1);
  bindStepper($("btn-down"), -1);

  // 页面隐藏：状态落盘 + 节拍按可见性收放（唤回立即补一拍）
  document.addEventListener("visibilitychange", () => { if (document.hidden) save(); else tick(); syncTick(); });

  document.addEventListener("desk-setting-changed", (e) => {
    const { key, value } = e.detail || {};
    if (key === "theme") applyTheme(value);
    if (key === "mode") setMode(value);
    if (key === "minutes") onMinutesChange();
    applyStatic();
    render();
    syncMenu();
    tick();
  });
  document.addEventListener("desk-language-changed", () => { applyStatic(); render(); syncMenu(); tick(); });

  const ringVal = $("ring-val");
  ringVal.style.strokeDasharray = `${CIRC}`;
  applyStatic();
  render();
  tick();
  // 先按空闲态首刷（不等存档 IO），存档回来若有进行中的计时会覆盖重绘
  //（render 内含 syncTick——节拍随相位自动收放，不再常驻裸 interval）
  load().then(() => { render(); syncMenu(); tick(); });
})();
