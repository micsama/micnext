// localStorage 在隐私模式等情况下可能不可用；不可用时只是不记住，不影响使用。
export function load(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** 缺失或内容损坏都视为未记住。 */
export function loadJson(key: string): unknown {
  try {
    return JSON.parse(load(key) ?? "null");
  } catch {
    return null;
  }
}

export function save(key: string, value: string | null): void {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // 同上
  }
}
