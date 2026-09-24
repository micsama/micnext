/** 路由只有两种：`/` 新会话、`/s/<id>` 某会话。 */
export type Route = { kind: "new" } | { kind: "session"; id: number };

function parse(path: string): Route {
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
