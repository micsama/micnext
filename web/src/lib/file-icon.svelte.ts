import { extensions, fallback, lightExtensions, lightNames, names, urls } from "virtual:file-icons";

const LIGHT = { names: { ...names, ...lightNames }, extensions: { ...extensions, ...lightExtensions } };
const DARK = { names, extensions };

/** 同 VS Code：完整文件名优先，其次从最长的多段扩展名（`test.ts`）到最短。 */
export function fileIconUrl(name: string, dark: boolean): string {
  const map = dark ? DARK : LIGHT;
  const lower = name.toLowerCase();
  let icon = map.names[lower];
  for (let i = lower.indexOf("."); !icon && i !== -1; i = lower.indexOf(".", i + 1)) {
    icon = map.extensions[lower.slice(i + 1)];
  }
  return urls[icon ?? fallback]!;
}
