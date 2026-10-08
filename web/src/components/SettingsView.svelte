<script lang="ts">
  import { untrack } from "svelte";
  import {
    createEndpoint,
    createModel,
    createPersona,
    deleteEndpoint,
    deleteModel,
    deletePersona,
    putSettings,
    setDefaultModel,
    testEndpoint,
    updateEndpoint,
    updateModel,
    updatePersona,
  } from "../api/client";
  import type { Credential, Endpoint, Persona, Preset, SettingsInput } from "../api/types";
  import Icon from "../lib/Icon.svelte";
  import { router } from "../state/route.svelte";
  import { settingsStore } from "../state/settings.svelte";
  import Header from "./Header.svelte";

  let { onmenu }: { onmenu: () => void } = $props();

  $effect(() => {
    document.title = "设置 · micnext";
  });

  // ---- 对话偏好 ----

  let prefs = $state<SettingsInput | null>(null);
  let prefsSaving = $state(false);
  let prefsMessage = $state<{ ok: boolean; text: string } | null>(null);

  const saved = $derived.by((): SettingsInput | null => {
    const s = settingsStore.settings;
    return s && {
      default_persona_id: s.default_persona_id,
      general_prompt: s.general_prompt,
      default_workdir: s.default_workdir,
      max_turns: s.max_turns,
    };
  });
  /** 表单载入时的服务端值。 */
  let prefsBase = $state<SettingsInput | null>(null);
  const same = (a: SettingsInput | null, b: SettingsInput | null) => JSON.stringify(a) === JSON.stringify(b);
  const prefsDirty = $derived(!same(prefs, prefsBase));

  // 未改动时跟随服务端最新值；改动中不覆盖。
  $effect(() => {
    const next = saved;
    untrack(() => {
      if (next && !prefsDirty && !same(next, prefsBase)) {
        prefs = { ...next };
        prefsBase = { ...next };
      }
    });
  });

  async function savePrefs() {
    if (!prefs) return;
    prefsSaving = true;
    prefsMessage = null;
    try {
      await putSettings({ ...prefs, max_turns: Number(prefs.max_turns) });
      await settingsStore.refresh();
      prefsMessage = { ok: true, text: "已保存，下一轮对话起生效" };
    } catch (e) {
      prefsMessage = { ok: false, text: (e as Error).message };
    } finally {
      prefsSaving = false;
    }
  }

  // ---- 人设 ----

  type Editing = { id: number | null; name: string; prompt: string };
  let editing = $state<Editing | null>(null);
  let editBase = $state<Editing | null>(null);
  let personaBusy = $state(false);
  let personaError = $state<string | null>(null);

  const personaDirty = $derived(
    !!editing && !!editBase && (editing.name !== editBase.name || editing.prompt !== editBase.prompt),
  );

  function open(e: Editing) {
    if (personaDirty && !confirm("人设有未保存的修改，确定放弃？")) return;
    editing = { ...e };
    editBase = { ...e };
    personaError = null;
  }

  function close() {
    editing = null;
    editBase = null;
    personaError = null;
  }

  async function persona(action: () => Promise<number | null>) {
    personaBusy = true;
    personaError = null;
    try {
      const id = await action();
      await settingsStore.refresh();
      const p = id === null ? undefined : settingsStore.persona(id);
      if (p) {
        editing = { id: p.id, name: p.name, prompt: p.prompt };
        editBase = { ...editing };
      } else close();
    } catch (e) {
      personaError = (e as Error).message;
    } finally {
      personaBusy = false;
    }
  }

  function savePersona() {
    const e = editing!;
    void persona(async () => {
      if (e.id === null) return createPersona(e.name, e.prompt);
      await updatePersona(e.id, e.name, e.prompt);
      return e.id;
    });
  }

  function removePersona(p: Persona) {
    if (!confirm(`删除人设「${p.name}」？已用它的历史记录不受影响，选着它的会话需要重新选择。`)) return;
    void persona(async () => {
      await deletePersona(p.id);
      return null;
    });
  }

  function copyPersona(p: Persona) {
    if (personaDirty && !confirm("人设有未保存的修改，确定放弃？")) return;
    const names = new Set(settingsStore.personas.map((x) => x.name));
    let name = `${p.name} 副本`;
    for (let i = 2; names.has(name); i++) name = `${p.name} 副本 ${i}`;
    void persona(() => createPersona(name, p.prompt));
  }

  // ---- 服务商与模型 ----

  const PRESETS: { value: Preset; label: string; hint: string }[] = [
    { value: "deepseek", label: "DeepSeek", hint: "地址固定为官方接口，无需填写" },
    { value: "ollama", label: "Ollama", hint: "留空用本机 http://localhost:11434/v1" },
    { value: "generic", label: "OpenAI 兼容（vLLM 等）", hint: "必填，如 http://host:8000/v1" },
  ];

  const PRESET_ENV: Record<Preset, string | null> = {
    deepseek: "DEEPSEEK_API_KEY",
    generic: "OPENAI_API_KEY",
    ollama: null,
  };

  const OLLAMA_URL = "http://localhost:11434/v1";

  type ModelRow = {
    /** 已存在的模型；新行为 null。 */
    id: number | null;
    name: string;
    max_tokens: string;
    reasoning_effort: string;
  };

  type EndpointForm = {
    id: number | null;
    name: string;
    preset: Preset;
    base_url: string;
    key_set: boolean;
    /** 新 key；留空即不改。 */
    key: string;
    clear_key: boolean;
    models: ModelRow[];
  };

  const blankEndpoint = (): EndpointForm => ({
    id: null,
    name: "",
    preset: "deepseek",
    base_url: "",
    key_set: false,
    key: "",
    clear_key: false,
    models: [],
  });

  const toForm = (e: Endpoint): EndpointForm => ({
    id: e.id,
    name: e.name,
    preset: e.config.preset,
    base_url: e.config.base_url === null || e.config.base_url === OLLAMA_URL ? "" : e.config.base_url,
    key_set: e.key_set,
    key: "",
    clear_key: false,
    models: settingsStore.models
      .filter((m) => m.endpoint_id === e.id)
      .map((m) => ({
        id: m.id,
        name: m.name,
        max_tokens: m.config.max_tokens === null ? "" : String(m.config.max_tokens),
        reasoning_effort: m.config.reasoning_effort ?? "",
      })),
  });

  let form = $state<EndpointForm | null>(null);
  let formBase = $state<EndpointForm | null>(null);
  let modelBusy = $state(false);
  let modelError = $state<string | null>(null);
  let testing = $state(false);
  let testMessage = $state<{ ok: boolean; text: string } | null>(null);

  const modelDirty = $derived(!!form && !!formBase && JSON.stringify(form) !== JSON.stringify(formBase));

  function openEndpoint(f: EndpointForm) {
    if (modelDirty && !confirm("服务商有未保存的修改，确定放弃？")) return;
    form = structuredClone($state.snapshot(f));
    formBase = structuredClone($state.snapshot(f));
    modelError = null;
    testMessage = null;
  }

  function closeEndpoint() {
    form = null;
    formBase = null;
    modelError = null;
    testMessage = null;
  }

  async function modelAction(action: () => Promise<void>) {
    modelBusy = true;
    modelError = null;
    try {
      await action();
    } catch (e) {
      modelError = (e as Error).message;
    } finally {
      await settingsStore.refresh();
      modelBusy = false;
    }
  }

  function credentialOf(f: EndpointForm): Credential {
    if (f.key.trim() !== "") return { op: "set", value: f.key };
    return f.clear_key || f.id === null || !f.key_set ? { op: "clear" } : { op: "keep" };
  }

  const configOf = (f: EndpointForm) => ({
    preset: f.preset,
    ...(f.preset !== "deepseek" && f.base_url.trim() && { base_url: f.base_url.trim() }),
  });

  async function runTest() {
    const f = form!;
    testing = true;
    testMessage = null;
    try {
      const names = await testEndpoint({
        kind: "openai",
        config: configOf(f),
        credential: credentialOf(f),
        ...(f.id !== null && { endpoint_id: f.id }),
      });
      const have = new Set(f.models.map((m) => m.name));
      const added = names.filter((n) => !have.has(n));
      f.models.push(...added.map((name) => ({ id: null, name, max_tokens: "", reasoning_effort: "" })));
      testMessage = { ok: true, text: `连接成功，共 ${names.length} 个模型，新增 ${added.length} 行` };
    } catch (e) {
      testMessage = { ok: false, text: (e as Error).message };
    } finally {
      testing = false;
    }
  }

  function modelInput(endpoint_id: number, r: ModelRow, preset: Preset) {
    const max = r.max_tokens.trim();
    return {
      endpoint_id,
      name: r.name.trim(),
      config: {
        ...(max && { max_tokens: Number(max) }),
        ...(preset === "deepseek" && r.reasoning_effort && { reasoning_effort: r.reasoning_effort }),
      },
    };
  }

  function saveEndpoint() {
    const f = $state.snapshot(form!);
    const base = formBase!;
    const input = { name: f.name, kind: "openai", config: configOf(f), credential: credentialOf(f) };
    void modelAction(async () => {
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
      closeEndpoint();
    });
  }

  function removeEndpoint(e: Endpoint) {
    if (!confirm(`删除服务商「${e.name}」？其下模型和已保存的 API key 一并删除，选着这些模型的会话需要重新选择。`)) return;
    void modelAction(() => deleteEndpoint(e.id));
  }

  const keyPlaceholder = (f: EndpointForm) =>
    f.key_set
      ? "已设置（输入新 key 替换）"
      : PRESET_ENV[f.preset]
        ? `API key（不填则读环境变量 ${PRESET_ENV[f.preset]}）`
        : "API key（Ollama 无需填写）";

  // ---- 离开提示 ----

  const dirty = $derived(prefsDirty || personaDirty || modelDirty);
  $effect(() => {
    router.dirty = () => dirty;
    const onbeforeunload = (e: BeforeUnloadEvent) => {
      if (dirty) e.preventDefault();
    };
    addEventListener("beforeunload", onbeforeunload);
    return () => {
      router.dirty = null;
      removeEventListener("beforeunload", onbeforeunload);
    };
  });

  const input =
    "w-full rounded-lg border border-line bg-bg px-3 py-2 text-sm outline-none focus:border-accent disabled:opacity-60";
  const button = "rounded-lg px-3 py-1.5 text-sm disabled:opacity-40";
</script>

<Header title="设置" {onmenu} />
<div class="min-h-0 flex-1 overflow-y-auto">
  <div class="mx-auto w-full max-w-3xl space-y-10 px-4 py-6">
    {#if settingsStore.error}
      <p class="text-sm text-danger">{settingsStore.error}</p>
    {/if}

    <section class="space-y-4">
      <h2 class="text-base font-semibold">对话偏好</h2>
      {#if prefs}
        <label class="block space-y-1">
          <span class="text-sm">默认人设</span>
          <select bind:value={prefs.default_persona_id} class={input}>
            {#each settingsStore.personas as p (p.id)}
              <option value={p.id}>{p.name}</option>
            {/each}
          </select>
          <span class="block text-xs text-muted">新会话默认使用；不影响已有会话。</span>
        </label>
        <label class="block space-y-1">
          <span class="text-sm">通用偏好</span>
          <textarea
            bind:value={prefs.general_prompt}
            rows="4"
            class={input}
            placeholder="对所有人设生效，如：回答尽量简短；我在上海，用公制单位。"></textarea>
        </label>
        <label class="block space-y-1">
          <span class="text-sm">新会话默认工作目录</span>
          <input bind:value={prefs.default_workdir} class="{input} font-mono" placeholder="~/workspace/mic" />
          <span class="block text-xs text-muted">绝对路径或 ~/ 开头，不存在会自动创建；已有会话的目录不变。</span>
        </label>
        <label class="block space-y-1">
          <span class="text-sm">单轮调用上限</span>
          <input type="number" min="1" max="500" bind:value={prefs.max_turns} class="{input} w-32" />
          <span class="block text-xs text-muted">一轮里带工具的模型调用次数，用尽后模型会总结已做的事。</span>
        </label>
        <div class="flex items-center gap-3">
          <button
            type="button"
            onclick={savePrefs}
            disabled={!prefsDirty || prefsSaving}
            class="{button} bg-accent text-accent-fg">保存</button>
          {#if prefsMessage && !(prefsMessage.ok && prefsDirty)}
            <span class="text-sm" class:text-danger={!prefsMessage.ok} class:text-muted={prefsMessage.ok}
              >{prefsMessage.text}</span>
          {:else if prefsDirty}
            <span class="text-sm text-warn">有未保存的修改</span>
          {/if}
        </div>
        {#if settingsStore.settings}
          <details class="rounded-lg border border-line bg-panel px-3 py-2">
            <summary class="cursor-pointer text-sm text-muted">系统提示词（内置，只读）</summary>
            <pre class="mt-2 font-mono text-xs whitespace-pre-wrap text-muted">{settingsStore.settings
                .system_prompt}</pre>
          </details>
        {/if}
      {:else if !settingsStore.error}
        <p class="text-sm text-muted">加载中…</p>
      {/if}
    </section>

    <section class="space-y-3">
      <div class="flex items-center justify-between">
        <h2 class="text-base font-semibold">人设</h2>
        <button
          type="button"
          onclick={() => open({ id: null, name: "", prompt: "" })}
          class="flex items-center gap-1 {button} border border-line hover:bg-raised">
          <Icon name="plus" />新建
        </button>
      </div>
      <p class="text-xs text-muted">人设决定助手的角色与语气，在输入框旁按会话选择。内置人设只读，可复制后编辑。</p>

      {#snippet editor(e: Editing)}
        <div class="space-y-2 rounded-lg border border-accent bg-bg p-3">
          <input bind:value={e.name} class={input} placeholder="名字" maxlength="40" />
          <textarea bind:value={e.prompt} rows="6" class={input} placeholder="提示词：描述角色、做事方式与语气"
          ></textarea>
          {#if personaError}
            <p class="text-sm text-danger">{personaError}</p>
          {/if}
          <div class="flex gap-2">
            <button
              type="button"
              onclick={savePersona}
              disabled={personaBusy || !personaDirty || !e.name.trim() || !e.prompt.trim()}
              class="{button} bg-accent text-accent-fg">保存</button>
            <button type="button" onclick={close} disabled={personaBusy} class="{button} hover:bg-raised"
              >{personaDirty ? "放弃修改" : "收起"}</button>
          </div>
        </div>
      {/snippet}

      {#if editing?.id === null}
        {@render editor(editing)}
      {/if}

      <ul class="space-y-2">
        {#each settingsStore.personas as p (p.id)}
          <li>
            {#if editing?.id === p.id}
              {@render editor(editing)}
            {:else}
              <div class="rounded-lg border border-line bg-panel p-3">
                <div class="flex items-center gap-2">
                  <span class="flex-1 truncate text-sm font-medium">
                    {p.name}
                    {#if p.builtin}<span class="ml-1 text-xs font-normal text-muted">内置</span>{/if}
                    {#if settingsStore.settings?.default_persona_id === p.id}
                      <span class="ml-1 text-xs font-normal text-muted">默认</span>
                    {/if}
                  </span>
                  {#if p.builtin}
                    <button
                      type="button"
                      onclick={() => copyPersona(p)}
                      disabled={personaBusy}
                      class="{button} text-xs text-muted hover:bg-raised hover:text-fg">复制并编辑</button>
                  {:else}
                    <button
                      type="button"
                      onclick={() => open({ id: p.id, name: p.name, prompt: p.prompt })}
                      disabled={personaBusy}
                      class="{button} text-xs text-muted hover:bg-raised hover:text-fg">编辑</button>
                    <button
                      type="button"
                      onclick={() => removePersona(p)}
                      disabled={personaBusy}
                      class="{button} text-xs text-danger hover:bg-raised">删除</button>
                  {/if}
                </div>
                <p class="mt-1 line-clamp-2 text-xs text-muted">{p.prompt}</p>
              </div>
            {/if}
          </li>
        {/each}
      </ul>
      {#if personaError && !editing}
        <p class="text-sm text-danger">{personaError}</p>
      {/if}
    </section>

    <section class="space-y-3">
      <div class="flex items-center justify-between">
        <h2 class="text-base font-semibold">服务商与模型</h2>
        <button
          type="button"
          onclick={() => openEndpoint(blankEndpoint())}
          class="flex items-center gap-1 {button} border border-line hover:bg-raised">
          <Icon name="plus" />添加服务商
        </button>
      </div>
      <p class="text-xs text-muted">
        API key 加密保存在本机数据库，主密钥在配置文件旁的 master.key；界面不会再显示已保存的 key。
      </p>

      {#snippet endpointEditor(f: EndpointForm)}
        <div class="space-y-2 rounded-lg border border-accent bg-bg p-3">
          <input bind:value={f.name} class={input} placeholder="服务商名称（在选择框里显示）" maxlength="40" />
          <select bind:value={f.preset} class={input} aria-label="类型">
            {#each PRESETS as p (p.value)}
              <option value={p.value}>{p.label}</option>
            {/each}
          </select>
          <label class="block space-y-1">
            {#if f.preset !== "deepseek"}
              <input bind:value={f.base_url} class="{input} font-mono" placeholder="服务地址" />
            {/if}
            <span class="block text-xs text-muted">{PRESETS.find((p) => p.value === f.preset)?.hint}</span>
          </label>
          <div class="space-y-1">
            <input
              type="password"
              autocomplete="off"
              bind:value={f.key}
              disabled={f.clear_key}
              class="{input} font-mono"
              placeholder={keyPlaceholder(f)} />
            {#if f.key_set}
              <label class="flex items-center gap-2 text-xs text-muted">
                <input type="checkbox" bind:checked={f.clear_key} onchange={() => (f.key = "")} />清除已保存的 key
              </label>
            {/if}
          </div>
          <div class="flex items-center gap-2">
            <button type="button" onclick={runTest} disabled={testing} class="{button} border border-line hover:bg-raised"
              >{testing ? "测试中…" : "测试并获取模型"}</button>
            {#if testMessage}
              <span class="text-xs {testMessage.ok ? 'text-muted' : 'text-danger'}">{testMessage.text}</span>
            {/if}
          </div>

          <div class="space-y-2 border-t border-line pt-2">
            <div class="flex items-center justify-between">
              <span class="text-sm font-medium">模型</span>
              <button
                type="button"
                onclick={() => f.models.push({ id: null, name: "", max_tokens: "", reasoning_effort: "" })}
                class="{button} text-xs text-muted hover:bg-raised hover:text-fg">添加一行</button>
            </div>
            {#each f.models as r, i (i)}
              <div class="flex flex-wrap items-center gap-2">
                <input bind:value={r.name} class="{input} min-w-40 flex-1 font-mono" placeholder="模型名" />
                <input
                  type="number"
                  min="1"
                  bind:value={r.max_tokens}
                  class="{input} w-32"
                  placeholder="最大输出" />
                {#if f.preset === "deepseek"}
                  <select bind:value={r.reasoning_effort} class="{input} w-32" aria-label="推理强度">
                    <option value="">推理：默认</option>
                    <option value="none">关闭思考</option>
                    <option value="low">low</option>
                    <option value="high">high</option>
                    <option value="max">max</option>
                  </select>
                {/if}
                <button
                  type="button"
                  onclick={() => f.models.splice(i, 1)}
                  class="{button} text-xs text-danger hover:bg-raised">移除</button>
              </div>
            {:else}
              <p class="text-xs text-muted">还没有模型：点「测试并获取模型」自动填入，或手动添加一行。</p>
            {/each}
          </div>

          {#if modelError}
            <p class="text-sm text-danger">{modelError}</p>
          {/if}
          <div class="flex gap-2">
            <button
              type="button"
              onclick={saveEndpoint}
              disabled={modelBusy || !modelDirty || !f.name.trim()}
              class="{button} bg-accent text-accent-fg">保存</button>
            <button type="button" onclick={closeEndpoint} disabled={modelBusy} class="{button} hover:bg-raised"
              >{modelDirty ? "放弃修改" : "收起"}</button>
          </div>
        </div>
      {/snippet}

      {#if form?.id === null}
        {@render endpointEditor(form)}
      {/if}

      <ul class="space-y-2">
        {#each settingsStore.endpoints as e (e.id)}
          <li>
            {#if form?.id === e.id}
              {@render endpointEditor(form)}
            {:else}
              <div class="rounded-lg border border-line bg-panel p-3">
                <div class="flex items-center gap-2">
                  <span class="flex-1 truncate text-sm font-medium">{e.name}</span>
                  <button
                    type="button"
                    onclick={() => openEndpoint(toForm(e))}
                    disabled={modelBusy}
                    class="{button} text-xs text-muted hover:bg-raised hover:text-fg">编辑</button>
                  <button
                    type="button"
                    onclick={() => removeEndpoint(e)}
                    disabled={modelBusy}
                    class="{button} text-xs text-danger hover:bg-raised">删除</button>
                </div>
                <p class="mt-1 truncate font-mono text-xs text-muted">
                  {e.config.base_url ?? "api.deepseek.com"} ·
                  {e.key_set ? "key 已设置" : e.key_env ? `读环境变量 ${e.key_env}` : "无 key"}
                </p>
                <ul class="mt-2 space-y-1">
                  {#each settingsStore.models.filter((m) => m.endpoint_id === e.id) as m (m.id)}
                    <li class="flex items-center gap-2 text-sm">
                      <span class="flex-1 truncate font-mono text-xs">
                        {m.name}
                        {#if settingsStore.defaultModelId === m.id}
                          <span class="ml-1 font-sans text-muted">默认</span>
                        {/if}
                      </span>
                      {#if settingsStore.defaultModelId !== m.id}
                        <button
                          type="button"
                          onclick={() => modelAction(() => setDefaultModel(m.id))}
                          disabled={modelBusy}
                          class="{button} text-xs text-muted hover:bg-raised hover:text-fg">设为默认</button>
                      {/if}
                    </li>
                  {:else}
                    <li class="text-xs text-muted">还没有模型</li>
                  {/each}
                </ul>
              </div>
            {/if}
          </li>
        {/each}
      </ul>
      {#if settingsStore.loaded && settingsStore.endpoints.length === 0 && form === null}
        <p class="text-sm text-muted">还没有服务商，点右上角「添加服务商」。</p>
      {/if}
      {#if modelError && !form}
        <p class="text-sm text-danger">{modelError}</p>
      {/if}
    </section>
  </div>
</div>
