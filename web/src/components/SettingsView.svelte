<script lang="ts">
  import { untrack } from "svelte";
  import { createPersona, deletePersona, putSettings, updatePersona } from "../api/client";
  import type { Persona, SettingsInput } from "../api/types";
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

  // ---- 离开提示 ----

  const dirty = $derived(prefsDirty || personaDirty);
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

    <section class="space-y-2">
      <h2 class="text-base font-semibold">模型</h2>
      <p class="text-sm text-muted">暂在配置文件 [models] 里设置，改完重启生效；之后会移到这里。</p>
    </section>
  </div>
</div>
