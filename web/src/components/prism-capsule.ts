/**
 * [INPUT]: 依赖导航组的局部几何与浏览器帧调度。
 * [OUTPUT]: 提供可中断、保留速度且收敛停帧的玻璃胶囊。
 * [POS]: PrismInteractions 的装饰层；只变换胶囊，不移动业务控件。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
type Geometry = [number, number, number, number];

export function createPrismCapsule(group: HTMLElement, className: string) {
  const element = document.createElement("span");
  element.className = className;
  element.setAttribute("aria-hidden", "true");
  group.prepend(element);
  let current: Geometry | undefined;
  let target: Geometry = [0, 0, 1, 1];
  const velocity: Geometry = [0, 0, 0, 0];
  let frame = 0;
  let previous = 0;
  let started = 0;

  function paint() {
    if (!current) return;
    // 固定底板尺寸；尺寸插值只在合成层缩放。
    element.style.width = `${target[2]}px`;
    element.style.height = `${target[3]}px`;
    element.style.transform = `translate3d(${current[0]}px,${current[1]}px,0) scale(${current[2] / target[2]},${current[3] / target[3]})`;
  }
  function settle() {
    cancelAnimationFrame(frame);
    frame = 0;
    current = [...target];
    velocity.fill(0);
    element.removeAttribute("data-prism-moving");
    paint();
  }
  function step(time: number) {
    if (!element.isConnected || document.hidden) { settle(); return; }
    const elapsed = Math.min((time - previous) / 1000, .032);
    previous = time;
    const steps = Math.max(1, Math.ceil(elapsed / .008));
    const dt = elapsed / steps;
    for (let n = 0; n < steps; n++) {
      for (let i = 0; i < 4; i++) {
        velocity[i] += ((target[i] - current![i]) * 340 - velocity[i] * 29) * dt;
        current![i] += velocity[i] * dt;
      }
    }
    const stopped = current!.every((value, i) => Math.abs(value - target[i]) < .08 && Math.abs(velocity[i]) < .8);
    if (stopped || time - started > 1200) { settle(); return; }
    paint();
    frame = requestAnimationFrame(step);
  }
  return {
    element,
    move(control: HTMLElement, animate: boolean) {
      const bounds = group.getBoundingClientRect();
      const rect = control.getBoundingClientRect();
      if (rect.width < 1 || rect.height < 1) { this.hide(); return; }
      const next: Geometry = [rect.left - bounds.left + group.scrollLeft - group.clientLeft, rect.top - bounds.top + group.scrollTop - group.clientTop, rect.width, rect.height];
      const changed = next.some((value, i) => Math.abs(value - target[i]) > .1);
      target = next;
      element.classList.add("visible");
      if (!current || !animate) { settle(); return; }
      if (!changed) return;
      started = performance.now();
      paint();
      element.dataset.prismMoving = "";
      if (frame) return;
      previous = started = performance.now();
      frame = requestAnimationFrame(step);
    },
    hide() { element.classList.remove("visible"); settle(); },
    settle,
    destroy() { cancelAnimationFrame(frame); element.remove(); },
  };
}
