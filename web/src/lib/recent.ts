import { loadJson, save } from "./storage";

/** 浏览器本地的「最近使用」列表：去重提前、限长；存储不可用时仅本次有效。 */
export function loadRecent(key: string): string[] {
  const v = loadJson(key);
  return Array.isArray(v) ? v.filter((s): s is string => typeof s === "string") : [];
}

export function pushRecent(key: string, list: string[], item: string, max: number): string[] {
  const next = [item, ...list.filter((s) => s !== item)].slice(0, max);
  save(key, JSON.stringify(next));
  return next;
}
