import { extensions, fallback, lightExtensions, lightNames, names } from "virtual:file-icons";

const LIGHT = { names: { ...names, ...lightNames }, extensions: { ...extensions, ...lightExtensions } };
const DARK = { names, extensions };

/** 图标 data URL 合在一个 chunk，首次用到时加载。 */
const icons = $state<{ urls?: Record<string, string> }>({});
let requested = false;

/** 同 VS Code：完整文件名优先，其次从最长的多段扩展名（`test.ts`）到最短。图标未加载完时为 `undefined`。 */
export function fileIconUrl(name: string, dark: boolean): string | undefined {
  if (!requested) {
    requested = true;
    void import("virtual:file-icon-svgs").then((m) => (icons.urls = m.default));
  }
  const map = dark ? DARK : LIGHT;
  const lower = name.toLowerCase();
  let icon = map.names[lower];
  for (let i = lower.indexOf("."); !icon && i !== -1; i = lower.indexOf(".", i + 1)) {
    icon = map.extensions[lower.slice(i + 1)];
  }
  return icons.urls?.[icon ?? fallback];
}
