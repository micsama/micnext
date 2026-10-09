import {
  createEndpoint,
  createModel,
  deleteModel,
  testEndpoint,
  updateEndpoint,
  updateModel,
} from "../../api/client";
import type { Credential, Endpoint, EndpointInput, Model, ModelInput, Preset } from "../../api/types";

export const PRESETS: { value: Preset; label: string }[] = [
  { value: "deepseek", label: "DeepSeek" },
  { value: "ollama", label: "Ollama" },
  { value: "generic", label: "OpenAI 兼容" },
];

export const presetLabel = (p: Preset): string => PRESETS.find((x) => x.value === p)!.label;

/** 未填 key 时服务端读取的环境变量。 */
export const PRESET_ENV: Record<Preset, string | null> = {
  deepseek: "DEEPSEEK_API_KEY",
  generic: "OPENAI_API_KEY",
  ollama: null,
};

export const OLLAMA_URL = "http://localhost:11434/v1";

/** `default` 即不下发，交给服务端默认。 */
export const REASONING: { value: string; label: string }[] = [
  { value: "default", label: "默认" },
  { value: "none", label: "关闭思考" },
  { value: "low", label: "low" },
  { value: "high", label: "high" },
  { value: "max", label: "max" },
];

export type ModelRow = {
  /** 新行为 null。 */
  id: number | null;
  name: string;
  max_tokens: string;
  reasoning_effort: string;
};

export type EndpointForm = {
  id: number | null;
  name: string;
  preset: Preset;
  base_url: string;
  key_set: boolean;
  /** 留空即不改。 */
  key: string;
  clear_key: boolean;
  models: ModelRow[];
};

export const blankEndpoint = (): EndpointForm => ({
  id: null,
  name: "",
  preset: "deepseek",
  base_url: "",
  key_set: false,
  key: "",
  clear_key: false,
  models: [],
});

export const blankRow = (name = ""): ModelRow => ({ id: null, name, max_tokens: "", reasoning_effort: "default" });

export const toForm = (e: Endpoint, models: Model[]): EndpointForm => ({
  id: e.id,
  name: e.name,
  preset: e.config.preset,
  base_url: e.config.base_url === null || e.config.base_url === OLLAMA_URL ? "" : e.config.base_url,
  key_set: e.key_set,
  key: "",
  clear_key: false,
  models: models
    .filter((m) => m.endpoint_id === e.id)
    .map((m) => ({
      id: m.id,
      name: m.name,
      max_tokens: m.config.max_tokens === null ? "" : String(m.config.max_tokens),
      reasoning_effort: m.config.reasoning_effort ?? "default",
    })),
});

function credentialOf(f: EndpointForm): Credential {
  if (f.key.trim() !== "") return { op: "set", value: f.key };
  return f.clear_key || f.id === null || !f.key_set ? { op: "clear" } : { op: "keep" };
}

const configOf = (f: EndpointForm): EndpointInput["config"] => ({
  preset: f.preset,
  ...(f.preset !== "deepseek" && f.base_url.trim() && { base_url: f.base_url.trim() }),
});

function modelInput(endpoint_id: number, r: ModelRow, preset: Preset): ModelInput {
  const max = r.max_tokens.trim();
  return {
    endpoint_id,
    name: r.name.trim(),
    config: {
      ...(max && { max_tokens: Number(max) }),
      ...(preset === "deepseek" && r.reasoning_effort !== "default" && { reasoning_effort: r.reasoning_effort }),
    },
  };
}

/** 用当前表单连接服务商，返回其报告的模型名。 */
export function probe(f: EndpointForm): Promise<string[]> {
  return testEndpoint({
    kind: "openai",
    config: configOf(f),
    credential: credentialOf(f),
    ...(f.id !== null && { endpoint_id: f.id }),
  });
}

/** 按与 `base` 的差异写回服务商及其模型；切换预设时重写全部模型以套用新的参数规则。 */
export async function saveForm(f: EndpointForm, base: EndpointForm): Promise<void> {
  const input: EndpointInput = { name: f.name, kind: "openai", config: configOf(f), credential: credentialOf(f) };
  let id = f.id;
  if (id === null) id = await createEndpoint(input);
  else await updateEndpoint(id, input);
  const rows = f.models.filter((r) => r.name.trim());
  const baseRows = new Map(base.models.map((r) => [r.id, r]));
  for (const r of rows) {
    if (r.id === null) await createModel(modelInput(id, r, f.preset));
    else if (JSON.stringify(r) !== JSON.stringify(baseRows.get(r.id)) || f.preset !== base.preset)
      await updateModel(r.id, modelInput(id, r, f.preset));
  }
  const kept = new Set(rows.map((r) => r.id));
  for (const r of base.models) if (!kept.has(r.id)) await deleteModel(r.id!);
}
