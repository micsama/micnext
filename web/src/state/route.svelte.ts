/** `/` 新会话、`/s/<id>[?m=<消息>]` 某会话（可定位到某条消息）、`/settings[/models/<id>]` 设置（可直达某模型单价）、`/developer[/<页签>]` 开发者诊断。 */
export type DeveloperTab = "logs" | "sql" | "update";
export type Route =
  | { kind: "new" }
  | { kind: "schedules" }
  | { kind: "session"; id: number; message: number | null }
  | { kind: "settings"; model: number | null }
  | { kind: "trace"; id: number | null; run: number | null }
  | { kind: "developer"; tab: DeveloperTab };

function parse(path: string): Route {
  const url = new URL(path, location.origin);
  path = url.pathname;
  if (/^\/schedules\/?$/.test(path)) return { kind: "schedules" };
  const settings = /^\/settings(?:\/models\/(\d+))?\/?$/.exec(path);
  if (settings) return { kind: "settings", model: settings[1] ? Number(settings[1]) : null };
  const trace = /^\/(?:s\/(\d+)\/)?trace\/?$/.exec(path);
  if (trace) {
    const run = url.searchParams.get("run");
    return { kind: "trace", id: trace[1] ? Number(trace[1]) : null, run: run && /^\d+$/.test(run) ? Number(run) : null };
  }
  const dev = /^\/developer(?:\/(logs|sql|update))?\/?$/.exec(path);
  if (dev) return { kind: "developer", tab: (dev[1] ?? "logs") as DeveloperTab };
  const m = /^\/s\/(\d+)\/?$/.exec(path);
  const message = url.searchParams.get("m");
  return m?.[1] ? { kind: "session", id: Number(m[1]), message: message && /^\d+$/.test(message) ? Number(message) : null } : { kind: "new" };
}

class Router {
  current = $state<Route>(parse(location.pathname + location.search));

  constructor() {
    addEventListener("popstate", () => (this.current = parse(location.pathname + location.search)));
  }

  go(path: string): void {
    if (location.pathname + location.search !== path) history.pushState(null, "", path);
    this.current = parse(path);
  }

  replace(path: string): void {
    history.replaceState(null, "", path);
    this.current = parse(path);
  }
}

export const router = new Router();
