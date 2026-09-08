// 屿族模板：演示基座四件事——读设置 / accent 落变量 / i18n / 跨零点重算。
// 新皮肤复制本文件夹后：改 skin.json 的 id/name/档位尺寸，再写这里。
(() => {
  const s = Isles.settings();
  Isles.setAccent(s.accent);

  const $ = (id) => document.getElementById(id);

  function render() {
    const now = new Date();
    const start = new Date(now.getFullYear(), 0, 1);
    const dayOfYear = Math.floor((now - start) / 86400000) + 1;
    $("title").textContent = s.title || Isles.t("今年第几天", "Day of year");
    $("num").textContent = dayOfYear;
    $("unit").textContent = Isles.t("天", "d");
  }

  render();
  Isles.onMidnight(render);
})();
