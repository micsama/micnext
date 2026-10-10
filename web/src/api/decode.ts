// 边界解码：服务端数据逐字段校验成强类型，形状不符即抛 `ProtocolError`。

/** 服务端返回的形状与本界面不符：前后端版本不一致。 */
export class ProtocolError extends Error {
  constructor(what: string) {
    super(`界面与服务版本不一致，请刷新页面（${what}）`);
  }
}

export type Decoder<T> = (v: unknown, at: string) => T;

const fail = (at: string): never => {
  throw new ProtocolError(at);
};

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

export const str: Decoder<string> = (v, at) => (typeof v === "string" ? v : fail(at));

export const num: Decoder<number> = (v, at) => (typeof v === "number" ? v : fail(at));

export const bool: Decoder<boolean> = (v, at) => (typeof v === "boolean" ? v : fail(at));

export const any: Decoder<unknown> = (v) => v;

export const nullable =
  <T>(d: Decoder<T>): Decoder<T | null> =>
  (v, at) =>
    v === null ? null : d(v, at);

export const arr =
  <T>(d: Decoder<T>): Decoder<T[]> =>
  (v, at) =>
    Array.isArray(v) ? v.map((x, i) => d(x, `${at}[${i}]`)) : fail(at);

export const oneOf =
  <const L extends string>(...values: L[]): Decoder<L> =>
  (v, at) =>
    values.includes(v as L) ? (v as L) : fail(at);

/** 字段集合必须恰好一致。 */
export function obj<T extends Record<string, unknown>>(fields: { [K in keyof T]: Decoder<T[K]> }): Decoder<T> {
  return (v, at) => {
    if (!isRecord(v)) return fail(at);
    const out: Record<string, unknown> = {};
    for (const [k, d] of Object.entries<Decoder<unknown>>(fields)) {
      if (!Object.hasOwn(v, k)) return fail(`${at}.${k}`);
      out[k] = d(v[k], `${at}.${k}`);
    }
    const extra = Object.keys(v).find((k) => !Object.hasOwn(fields, k));
    return extra === undefined ? (out as T) : fail(`${at}.${extra}`);
  };
}

/** serde 外部标签枚举：`{ "Variant": payload }`。 */
export function tagged<M extends Record<string, unknown>>(variants: {
  [K in keyof M]: Decoder<M[K]>;
}): Decoder<{ [K in keyof M]: { [P in K]: M[K] } }[keyof M]> {
  return (v, at) => {
    if (!isRecord(v)) return fail(at);
    const entries = Object.entries(v);
    const [k, payload] = entries.length === 1 ? entries[0]! : fail(at);
    const d = Object.hasOwn(variants, k) ? (variants as Record<string, Decoder<unknown>>)[k]! : fail(`${at}.${k}`);
    return { [k]: d(payload, `${at}.${k}`) } as never;
  };
}

/** serde 内部标签枚举：`{ "kind": "Variant", ...fields }`。 */
export function kinded<M extends Record<string, Record<string, unknown>>>(variants: {
  [K in keyof M]: { [F in keyof M[K]]: Decoder<M[K][F]> };
}): Decoder<{ [K in keyof M]: { kind: K } & M[K] }[keyof M]> {
  return (v, at) => {
    if (!isRecord(v)) return fail(at);
    const { kind, ...rest } = v;
    const fields = typeof kind === "string" && Object.hasOwn(variants, kind)
      ? (variants as Record<string, Record<string, Decoder<unknown>>>)[kind]!
      : fail(`${at}.kind`);
    return { kind, ...obj(fields)(rest, at) } as never;
  };
}
