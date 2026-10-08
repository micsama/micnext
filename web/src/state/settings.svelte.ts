import { getSettings, listPersonas } from "../api/client";
import type { Persona, Settings } from "../api/types";

/** 对话偏好与未删除的人设；登录后加载，编辑后刷新。 */
class SettingsStore {
  settings = $state<Settings | null>(null);
  personas = $state<Persona[]>([]);
  error = $state<string | null>(null);

  get loaded(): boolean {
    return this.settings !== null;
  }

  async refresh(): Promise<void> {
    try {
      [this.settings, this.personas] = await Promise.all([getSettings(), listPersonas()]);
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
}

export const settingsStore = new SettingsStore();
