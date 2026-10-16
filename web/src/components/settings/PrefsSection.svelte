<script lang="ts">
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import { untrack } from "svelte";
  import * as Collapsible from "$lib/components/ui/collapsible";
  import { Input } from "$lib/components/ui/input";
  import * as Select from "$lib/components/ui/select";
  import { Textarea } from "$lib/components/ui/textarea";
  import { putSettings } from "../../api/client";
  import type { Settings, SettingsInput } from "../../api/types";
  import { settingsStore } from "../../state/settings.svelte";
  import PersonaIcon from "../PersonaIcon.svelte";
  import Group from "./Group.svelte";
  import SaveStatus, { type SaveState } from "./SaveStatus.svelte";
  import TitleModelSection from "./TitleModelSection.svelte";

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

{#snippet label(text: string, field: Field)}
  <div class="flex items-center gap-2">
    <span class="text-sm">{text}</span>
    <SaveStatus state={status[field]} />
  </div>
{/snippet}

{#snippet error(field: Field)}
  {@const s = status[field]}
  {#if s?.kind === "error"}
    <p class="text-xs text-destructive">{s.message}</p>
  {/if}
{/snippet}

<div class="space-y-8">
  <Group footer="改动即时保存，下一轮对话生效">
    <div class="space-y-1 px-4 py-3">
      <div class="flex items-center justify-between gap-4">
        {@render label("默认人设", "default_persona_id")}
        <Select.Root
          type="single"
          value={String(draft.default_persona_id)}
          onValueChange={(v) => save("default_persona_id", Number(v))}
        >
          <Select.Trigger class="w-44">
            {@const current = settingsStore.persona(draft.default_persona_id)}
            <span class="flex min-w-0 items-center gap-2">
              {#if current}<PersonaIcon persona={current} />{/if}
              <span class="truncate">{current?.name ?? "未选择"}</span>
            </span>
          </Select.Trigger>
          <Select.Content>
            {#each settingsStore.personas as p (p.id)}
              <Select.Item value={String(p.id)} label={p.name}><PersonaIcon persona={p} />{p.name}</Select.Item>
            {/each}
          </Select.Content>
        </Select.Root>
      </div>
      {@render error("default_persona_id")}
    </div>

    <div class="space-y-1 px-4 py-3">
      <div class="flex items-center justify-between gap-4">
        {@render label("工作目录", "default_workdir")}
        <Input
          class="w-64 font-mono text-xs"
          placeholder="~/workspace"
          bind:value={draft.default_workdir}
          onfocus={() => (editing = "default_workdir")}
          onblur={() => save("default_workdir", draft.default_workdir.trim())}
          onkeydown={blurOnEnter}
        />
      </div>
      {@render error("default_workdir")}
    </div>

    <div class="space-y-1 px-4 py-3">
      <div class="flex items-center justify-between gap-4">
        {@render label("最大轮数", "max_turns")}
        <Input
          type="number"
          min="1"
          class="w-24 text-right tabular-nums"
          value={draft.max_turns}
          onfocus={() => (editing = "max_turns")}
          onchange={(e) => save("max_turns", Number(e.currentTarget.value))}
          onblur={() => (editing = null)}
          onkeydown={blurOnEnter}
        />
      </div>
      {@render error("max_turns")}
    </div>

    <div class="space-y-2 px-4 py-3">
      {@render label("上下文压缩", "compaction")}
      <div class="flex flex-wrap items-center gap-3 text-sm text-muted-foreground">
        <label for="compaction-percent">触发比例（%）</label>
        <Input id="compaction-percent" type="number" min="1" max="80" class="w-24" value={draft.compaction.trigger_percent} onfocus={() => (editing = "compaction")} onchange={(e) => save("compaction", { ...draft.compaction, trigger_percent: Number(e.currentTarget.value) })} onblur={() => (editing = null)} />
        <label for="compaction-steps">保留步数</label>
        <Input id="compaction-steps" type="number" min="0" max="100" class="w-24" value={draft.compaction.keep_steps} onfocus={() => (editing = "compaction")} onchange={(e) => save("compaction", { ...draft.compaction, keep_steps: Number(e.currentTarget.value) })} onblur={() => (editing = null)} />
      </div>
      {@render error("compaction")}
    </div>

    <div class="space-y-2 px-4 py-3">
      {@render label("通用指令", "general_prompt")}
      <Textarea
        rows={4}
        placeholder="例如：回答尽量简短，使用中文。"
        bind:value={draft.general_prompt}
        onfocus={() => (editing = "general_prompt")}
        onblur={() => save("general_prompt", draft.general_prompt)}
      />
      {@render error("general_prompt")}
    </div>
  </Group>

  <TitleModelSection />

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
