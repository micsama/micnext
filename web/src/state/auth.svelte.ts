import { load, save } from "./storage";

const KEY = "micnext.token";

/** 取走地址里的 `#token=`：存下并从地址栏抹掉。 */
function takeHashToken(): string | null {
  const m = /(?:^#|&)token=([^&]+)/.exec(location.hash);
  if (!m?.[1]) return null;
  const token = decodeURIComponent(m[1]);
  save(KEY, token);
  history.replaceState(history.state, "", location.pathname + location.search);
  return token;
}

class Auth {
  token = $state<string | null>(takeHashToken() ?? load(KEY));

  constructor() {
    // 同一标签页粘贴新地址只改 hash，不会重新加载页面。
    addEventListener("hashchange", () => {
      const token = takeHashToken();
      if (token) this.token = token;
    });
  }

  /** 服务端返回 401：token 失效（如重启后随机 token 变了）。 */
  expire(): void {
    save(KEY, null);
    this.token = null;
  }
}

export const auth = new Auth();
