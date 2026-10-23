<script lang="ts">
  import { untrack } from "svelte";
  import { sendMessage } from "../api/client";
  import type { AskQuestion } from "../api/types";
  import type { SessionView } from "../state/session.svelte";

  let { view, replyId, questions }: { view: SessionView; replyId: number; questions: AskQuestion[] } = $props();

  const OTHER = -1;
  /** 每题选中的选项下标，`OTHER` 为「其它」；默认选中推荐项。入参不会变。 */
  let rows = $state(untrack(() => questions).map((q) => ({ q, picked: [0], other: "" })));
  type Row = (typeof rows)[number];
  let sending = $state(false);
  let error = $state<string | null>(null);

  const answerable = $derived(!view.messages.some((m) => m.id > replyId && m.body.kind === "UserInput"));
  const ready = $derived(rows.every((r) => r.picked.length > 0 && (!r.picked.includes(OTHER) || r.other.trim() !== "")));

  function toggle(row: Row, option: number) {
    if (!row.q.multi_select) row.picked = [option];
    else row.picked = row.picked.includes(option) ? row.picked.filter((o) => o !== option) : [...row.picked, option];
  }

  async function submit() {
    const text = rows
      .map(({ q, picked, other }) => {
        const answers = q.options.flatMap((o, n) => (picked.includes(n) ? [o.label] : []));
        if (picked.includes(OTHER)) answers.push(`其它：${other.trim()}`);
        return `「${q.question}」→ ${answers.join("、")}`;
      })
      .join("\n");
    sending = true;
    error = null;
    try {
      await sendMessage(view.id, [{ kind: "text", text }]);
    } catch (e) {
      error = (e as Error).message;
    } finally {
      sending = false;
    }
  }
</script>

<div class="my-2 space-y-2">
  {#each rows as row, i (i)}
    {@const q = row.q}
    <fieldset class="rounded-lg border border-border p-3" disabled={!answerable || sending}>
      <legend class="sr-only">{q.header}</legend>
      <p class="text-xs font-medium text-muted-foreground">{q.header}{q.multi_select ? " · 可多选" : ""}</p>
      <p class="mt-0.5 text-sm font-medium">{q.question}</p>
      <div class="mt-2 space-y-1">
        {#each [...q.options.map((o, n) => ({ n, label: o.label, description: o.description })), { n: OTHER, label: "其它", description: "" }] as option (option.n)}
          {@const on = answerable && row.picked.includes(option.n)}
          <button
            type="button"
            class={["flex w-full items-start gap-2 rounded-md border px-2.5 py-1.5 text-left text-sm", on ? "border-primary bg-primary/5" : "border-transparent", answerable && "hover:bg-muted"]}
            onclick={() => toggle(row, option.n)}>
            <span class="w-4 shrink-0 text-muted-foreground tabular-nums">{option.n === OTHER ? "·" : option.n + 1}</span>
            <span class="min-w-0">
              <span class="block">{option.label}</span>
              {#if option.description}<span class="block text-xs text-muted-foreground">{option.description}</span>{/if}
            </span>
          </button>
        {/each}
        {#if answerable && row.picked.includes(OTHER)}
          <input class="mt-1 w-full rounded-md border border-border bg-background px-2.5 py-1.5 text-sm" placeholder="写下你的回答" bind:value={row.other} />
        {/if}
      </div>
    </fieldset>
  {/each}
  {#if answerable}
    <div class="flex items-center gap-3">
      <button type="button" class="rounded-lg bg-primary px-4 py-1.5 text-sm text-primary-foreground disabled:opacity-50" disabled={!ready || sending} onclick={submit}>{sending ? "提交中…" : "提交"}</button>
      {#if error}<span class="text-sm text-destructive">{error}</span>{/if}
    </div>
  {/if}
</div>
