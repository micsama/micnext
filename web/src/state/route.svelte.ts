/** `/` 新会话、`/s/<id>` 某会话、`/settings` 设置、`/developer[/<页签>]` 开发者诊断。 */
export type DeveloperTab = "logs" | "sql" | "update";
export type Route =
  | { kind: "new" }
  | { kind: "session"; id: number }
  | { kind: "settings" }
  | { kind: "trace"; id: number | null }
  | { kind: "developer"; tab: DeveloperTab };

function parse(path: string): Route {
  if (/^\/settings\/?$/.test(path)) return { kind: "settings" };
  const trace = /^\/(?:s\/(\d+)\/)?trace\/?$/.exec(path);
  if (trace) return { kind: "trace", id: trace[1] ? Number(trace[1]) : null };
  const dev = /^\/developer(?:\/(logs|sql|update))?\/?$/.exec(path);
  if (dev) return { kind: "developer", tab: (dev[1] ?? "logs") as DeveloperTab };
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
