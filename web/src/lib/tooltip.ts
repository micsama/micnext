// 全站悬停说明：接管 title 属性，统一样式并缩短出现延迟；触屏不弹。
const DELAY = 250;
const GAP = 6;

export function installTooltips(): void {
  const tip = document.createElement("div");
  tip.className = "app-tooltip";
  tip.setAttribute("role", "tooltip");
  tip.hidden = true;
  document.body.append(tip);

  let target: HTMLElement | null = null;
  let timer = 0;

  const hide = () => {
    clearTimeout(timer);
    tip.hidden = true;
    if (!target) return;
    // 悬停期间框架可能已改写 title，以新值为准。
    if (!target.title) target.title = target.dataset.tip ?? "";
    delete target.dataset.tip;
    target = null;
  };

  const show = (el: HTMLElement) => {
    tip.textContent = el.dataset.tip ?? "";
    tip.hidden = false;
    const r = el.getBoundingClientRect();
    const w = tip.offsetWidth;
    const h = tip.offsetHeight;
    const left = Math.min(Math.max(r.left + r.width / 2 - w / 2, GAP), innerWidth - w - GAP);
    const below = r.bottom + GAP + h <= innerHeight;
    tip.style.left = `${left}px`;
    tip.style.top = `${below ? r.bottom + GAP : r.top - GAP - h}px`;
  };

  document.addEventListener("pointerover", (e) => {
    const node = e.target as Element;
    if (target?.contains(node)) return;
    hide();
    if (e.pointerType !== "mouse") return;
    const el = node.closest<HTMLElement>("[title]");
    if (!el?.title) return;
    target = el;
    el.dataset.tip = el.title;
    el.removeAttribute("title");
    timer = window.setTimeout(() => el.isConnected && show(el), DELAY);
  });
  document.addEventListener("pointerout", (e) => {
    if (!e.relatedTarget) hide();
  });
  for (const type of ["pointerdown", "keydown", "scroll"]) document.addEventListener(type, hide, true);
}
