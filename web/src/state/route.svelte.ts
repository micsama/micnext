/** `/` 新会话、`/s/<id>` 某会话、`/settings` 设置、`/developer` 开发者诊断。 */
export type Route = { kind: "new" } | { kind: "session"; id: number } | { kind: "settings" } | { kind: "developer" };

function parse(path: string): Route {
  if (/^\/settings\/?$/.test(path)) return { kind: "settings" };
  if (/^\/developer\/?$/.test(path)) return { kind: "developer" };
  const m = /^\/s\/(\d+)\/?$/.exec(path);
  return m?.[1] ? { kind: "session", id: Number(m[1]) } : { kind: "new" };
}

class Router {
  current = $state<Route>(parse(location.pathname));

  constructor() {
    addEventListener("popstate", () => (this.current = parse(location.pathname)));
  }

  go(path: string): void {
    if (location.pathname !== path) history.pushState(null, "", path);
    this.current = parse(path);
  }
}

export const router = new Router();
