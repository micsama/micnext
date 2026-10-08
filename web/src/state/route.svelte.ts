/** `/` 新会话、`/s/<id>` 某会话、`/settings` 设置。 */
export type Route = { kind: "new" } | { kind: "session"; id: number } | { kind: "settings" };

function parse(path: string): Route {
  if (/^\/settings\/?$/.test(path)) return { kind: "settings" };
  const m = /^\/s\/(\d+)\/?$/.exec(path);
  return m?.[1] ? { kind: "session", id: Number(m[1]) } : { kind: "new" };
}

class Router {
  current = $state<Route>(parse(location.pathname));
  /** 当前页有未保存的修改时返回 true，离开前请用户确认。 */
  dirty: (() => boolean) | null = null;

  constructor() {
    addEventListener("popstate", () => (this.current = parse(location.pathname)));
  }

  go(path: string): void {
    if (this.dirty?.() && !confirm("有未保存的修改，确定离开？")) return;
    if (location.pathname !== path) history.pushState(null, "", path);
    this.current = parse(path);
  }
}

export const router = new Router();
