<script lang="ts">
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import { untrack } from "svelte";
  import * as Collapsible from "$lib/components/ui/collapsible";
  import { Input } from "$lib/components/ui/input";
  import { Textarea } from "$lib/components/ui/textarea";
  import { putSettings } from "../../api/client";
  import type { Settings, SettingsInput } from "../../api/types";
  import { settingsStore } from "../../state/settings.svelte";
  import PersonaPicker from "../PersonaPicker.svelte";
  import Group from "./Group.svelte";
  import SaveStatus, { type SaveState } from "./SaveStatus.svelte";
  import PushSection from "./PushSection.svelte";
  import UtilityModelSection from "./UtilityModelSection.svelte";

  type Field = keyof SettingsInput;

  let { settings }: { settings: Settings } = $props();

  const pick = (s: Settings): SettingsInput => ({
    default_persona_id: s.default_persona_id,
    general_prompt: s.general_prompt,
    default_workdir: s.default_workdir,
    max_turns: s.max_turns,
    compaction: { ...s.compaction },
  });

  let draft = $state(untrack(() => pick(settings)));
  let editing = $state<Field | null>(null);
  let status = $state<Partial<Record<Field, SaveState>>>({});
  const timers: Partial<Record<Field, ReturnType<typeof setTimeout>>> = {};

  // 正在输入的字段不被服务端值覆盖。
  $effect(() => {
    const next = pick(settings);
    untrack(() => {
      for (const k of Object.keys(next) as Field[]) if (k !== editing) (draft[k] as unknown) = next[k];
    });
  });

  async function save<K extends Field>(field: K, value: SettingsInput[K]) {
    editing = null;
    draft[field] = value;
    if (value === settings[field]) return;
    clearTimeout(timers[field]);
    status[field] = { kind: "saving" };
    try {
      await putSettings({ ...pick(settings), [field]: value });
      await settingsStore.refresh();
      status[field] = { kind: "saved" };
      timers[field] = setTimeout(() => delete status[field], 2000);
    } catch (e) {
      status[field] = { kind: "error", message: (e as Error).message };
    }
  }

  const blurOnEnter = (e: KeyboardEvent) => {
    if (e.key === "Enter" && !e.isComposing) (e.currentTarget as HTMLElement).blur();
  };
</script>

{#snippet label(text: string, hint: string, field?: Field)}
  <div class="min-w-0 flex-1">
    <div class="flex items-center gap-2">
      <span class="text-sm">{text}</span>
      {#if field}<SaveStatus state={status[field]} />{/if}
    </div>
    <p class="mt-0.5 text-xs text-muted-foreground">{hint}</p>
  </div>
{/snippet}

{#snippet error(field: Field)}
  {@const s = status[field]}
  {#if s?.kind === "error"}
    <p class="text-xs text-destructive">{s.message}</p>
  {/if}
{/snippet}

{#snippet number(id: string, value: number, unit: string, min: number, max: number | undefined, field: Field, commit: (v: number) => void)}
  <div class="relative w-28 shrink-0">
    <Input
      {id}
      type="number"
      {min}
      {max}
      class="pr-8 text-right tabular-nums"
      {value}
      onfocus={() => (editing = field)}
      onchange={(e) => commit(Number(e.currentTarget.value))}
      onblur={() => (editing = null)}
      onkeydown={blurOnEnter}
    />
    <span class="pointer-events-none absolute inset-y-0 right-3 flex items-center text-xs text-muted-foreground">{unit}</span>
  </div>
{/snippet}

<div class="space-y-8">
  <Group title="对话" footer="改动即时保存，下一轮对话生效">
    <div class="space-y-1 px-4 py-3.5">
      <div class="flex items-center justify-between gap-4">
        {@render label("默认人设", "新对话使用的人设", "default_persona_id")}
        <PersonaPicker
          value={draft.default_persona_id}
          side="bottom"
          align="end"
          onpick={(id) => save("default_persona_id", id)}
        />
      </div>
      {@render error("default_persona_id")}
    </div>

    <div class="space-y-2.5 px-4 py-3.5">
      {@render label("通用指令", "对所有人设生效，附加在人设提示词之后", "general_prompt")}
      <Textarea
        rows={4}
        class="leading-relaxed"
        placeholder="例如：回答尽量简短，使用中文。"
        bind:value={draft.general_prompt}
        onfocus={() => (editing = "general_prompt")}
        onblur={() => save("general_prompt", draft.general_prompt)}
      />
      {@render error("general_prompt")}
    </div>
  </Group>

  <Group title="执行">
    <div class="space-y-1 px-4 py-3.5">
      <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
        {@render label("工作目录", "新会话中文件与命令工具的起始目录", "default_workdir")}
        <Input
          class="w-64 max-w-full font-mono text-xs"
          placeholder="~/workspace"
          bind:value={draft.default_workdir}
          onfocus={() => (editing = "default_workdir")}
          onblur={() => save("default_workdir", draft.default_workdir.trim())}
          onkeydown={blurOnEnter}
        />
      </div>
      {@render error("default_workdir")}
    </div>

    <div class="space-y-1 px-4 py-3.5">
      <div class="flex items-center justify-between gap-4">
        {@render label("最大轮数", "一次回复最多调用模型的次数，用完即停止工具并总结", "max_turns")}
        {@render number("max-turns", draft.max_turns, "轮", 1, undefined, "max_turns", (v) => save("max_turns", v))}
      </div>
      {@render error("max_turns")}
    </div>
  </Group>

  <Group title="上下文压缩">
    {#snippet actions()}<SaveStatus state={status.compaction} />{/snippet}
    <div class="flex items-center justify-between gap-4 px-4 py-3.5">
      {@render label("触发比例", "上下文用到模型窗口的这个比例时自动压缩")}
      {@render number("compaction-percent", draft.compaction.trigger_percent, "%", 1, 80, "compaction", (v) => save("compaction", { ...draft.compaction, trigger_percent: v }))}
    </div>
    <div class="flex items-center justify-between gap-4 px-4 py-3.5">
      {@render label("保留原文", "压缩时按当前上下文长度的比例保留最近的原文，至少保留最新一步")}
      {@render number("compaction-keep", draft.compaction.keep_percent, "%", 0, 50, "compaction", (v) => save("compaction", { ...draft.compaction, keep_percent: v }))}
    </div>
    {#if status.compaction?.kind === "error"}
      <p class="px-4 py-3 text-xs text-destructive">{status.compaction.message}</p>
    {/if}
  </Group>

  <UtilityModelSection />

  <PushSection />


  <Collapsible.Root>
    <Collapsible.Trigger
      class="group flex items-center gap-1 px-1 text-xs font-medium text-muted-foreground hover:text-foreground"
    >
      <ChevronRightIcon class="size-3.5 transition-transform group-data-[state=open]:rotate-90" />
      高级
    </Collapsible.Trigger>
    <Collapsible.Content class="pt-2">
      <Group title="系统提示词（只读）">
        <pre class="max-h-96 overflow-auto px-4 py-3 font-mono text-xs leading-relaxed whitespace-pre-wrap text-muted-foreground">{settings.system_prompt}</pre>
      </Group>
    </Collapsible.Content>
  </Collapsible.Root>
</div>
