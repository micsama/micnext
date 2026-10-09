import { getInputLimits, getSettings, listEndpoints, listModels, listPersonas } from "../api/client";
import type { Endpoint, InputLimits, Model, Persona, Settings } from "../api/types";

/** 对话偏好、未删除的人设与模型；登录后加载，编辑后刷新。 */
class SettingsStore {
  settings = $state<Settings | null>(null);
  personas = $state<Persona[]>([]);
  endpoints = $state<Endpoint[]>([]);
  models = $state<Model[]>([]);
  defaultModelId = $state<number | null>(null);
  inputLimits = $state<InputLimits | null>(null);
  error = $state<string | null>(null);

  get loaded(): boolean {
    return this.settings !== null;
  }

  async refresh(): Promise<void> {
    try {
      const [settings, personas, endpoints, models, inputLimits] = await Promise.all([
        getSettings(),
        listPersonas(),
        listEndpoints(),
        listModels(),
        getInputLimits(),
      ]);
      this.inputLimits = inputLimits;
      this.settings = settings;
      this.personas = personas;
      this.endpoints = endpoints;
      this.models = models.items;
      this.defaultModelId = models.default_model_id;
      this.error = null;
    } catch (e) {
      this.error = (e as Error).message;
    }
  }

  /** 选中的人设已删除时落到默认人设，与服务端下一轮的取法一致。 */
  resolve(id: number): { id: number; replaced: boolean } {
    if (!this.loaded || this.persona(id)) return { id, replaced: false };
    return { id: this.settings!.default_persona_id, replaced: true };
  }

  /** 已删除或不存在返回 undefined。 */
  persona(id: number): Persona | undefined {
    return this.personas.find((p) => p.id === id);
  }

  endpoint(id: number): Endpoint | undefined {
    return this.endpoints.find((e) => e.id === id);
  }

  model(id: number): Model | undefined {
    return this.models.find((m) => m.id === id);
  }
}

export const settingsStore = new SettingsStore();
