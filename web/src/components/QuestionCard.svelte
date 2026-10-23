<script lang="ts">
  import { untrack } from "svelte";
  import { sendMessage } from "../api/client";
  import type { AskQuestion } from "../api/types";
  import Icon from "../lib/Icon.svelte";
  import type { SessionView } from "../state/session.svelte";

  let { view, replyId, questions }: { view: SessionView; replyId: number; questions: AskQuestion[] } = $props();

  const OTHER = -1;
  /** 一题一页，答完进下一题，最后一页汇总提交。入参不会变。 */
  let rows = $state(untrack(() => questions).map((q) => ({ q, picked: [] as number[], other: "" })));
  type Row = (typeof rows)[number];
  let step = $state(0);
  let sending = $state(false);
  let error = $state<string | null>(null);

  const answerable = $derived(!view.messages.some((m) => m.id > replyId && m.body.kind === "UserInput"));
  const done = (r: Row) => r.picked.length > 0 && (!r.picked.includes(OTHER) || r.other.trim() !== "");
  const answer = (r: Row) => {
    const labels = r.q.options.flatMap((o, n) => (r.picked.includes(n) ? [o.label] : []));
    if (r.picked.includes(OTHER)) labels.push(`其它：${r.other.trim()}`);
    return labels.join("、");
  };
  const reviewing = $derived(step === rows.length);
  const row = $derived(rows[step]);

  function next() {
    step = rows.findIndex((r, i) => i > step && !done(r));
    if (step === -1) step = rows.length;
  }

  function pick(r: Row, option: number) {
    if (r.q.multi_select) {
      r.picked = r.picked.includes(option) ? r.picked.filter((o) => o !== option) : [...r.picked, option];
      return;
    }
    r.picked = [option];
    if (option !== OTHER) next();
  }

  async function submit() {
    sending = true;
    error = null;
    try {
      const text = rows.map((r) => `「${r.q.question}」→ ${answer(r)}`).join("\n");
      await sendMessage(view.id, [{ kind: "text", text }]);
    } catch (e) {
      error = (e as Error).message;
    } finally {
      sending = false;
    }
  }
</script>

{#if !answerable}
  <div class="my-2 rounded-xl border border-border bg-muted/30 px-4 py-3 text-sm">
    <p class="mb-1.5 flex items-center gap-1.5 text-xs text-muted-foreground"><Icon name="check" class="size-3.5" />已回答</p>
    {#each rows as r, i (i)}
      <p class="truncate text-muted-foreground"><span class="text-foreground/80">{r.q.header}</span> · {r.q.question}</p>
    {/each}
  </div>
{:else}
  <div class="my-2 overflow-hidden rounded-xl border border-border bg-card shadow-sm">
    {#if rows.length > 1}
      <div class="flex items-center gap-1 overflow-x-auto border-b border-border px-3 py-2 [scrollbar-width:none]">
        {#each rows as r, i (i)}
          <button
            type="button"
            class={["inline-flex shrink-0 items-center gap-1 rounded-full px-2.5 py-1 text-xs transition-colors", i === step ? "bg-primary/10 font-medium text-primary" : "text-muted-foreground hover:bg-muted"]}
            onclick={() => (step = i)}>
            {#if done(r)}<Icon name="check" class="size-3" />{:else}<span class="tabular-nums">{i + 1}</span>{/if}
            {r.q.header}
          </button>
        {/each}
        <button
          type="button"
          disabled={!rows.every(done)}
          class={["ml-auto inline-flex shrink-0 items-center rounded-full px-2.5 py-1 text-xs transition-colors disabled:opacity-40", reviewing ? "bg-primary/10 font-medium text-primary" : "text-muted-foreground hover:bg-muted"]}
          onclick={() => (step = rows.length)}>提交</button>
      </div>
    {/if}

    {#if row}
      <div class="p-4">
        <p class="text-[15px] leading-snug font-medium">{row.q.question}</p>
        {#if row.q.multi_select}<p class="mt-1 text-xs text-muted-foreground">可多选</p>{/if}
        <div class="mt-3 space-y-1.5">
          {#each [...row.q.options.map((o, n) => ({ n, label: o.label, description: o.description })), { n: OTHER, label: "其它", description: "自己写回答" }] as option (option.n)}
            {@const on = row.picked.includes(option.n)}
            <button
              type="button"
              class={["flex w-full items-start gap-3 rounded-lg border px-3 py-2.5 text-left transition-colors", on ? "border-primary/50 bg-primary/5" : "border-border hover:bg-muted/60"]}
              onclick={() => pick(row, option.n)}>
              <span class={["mt-px grid size-5 shrink-0 place-items-center rounded-md text-xs tabular-nums", on ? "bg-primary text-primary-foreground" : "bg-muted text-muted-foreground"]}>
                {#if on}<Icon name="check" class="size-3" />{:else if option.n === OTHER}…{:else}{option.n + 1}{/if}
              </span>
              <span class="min-w-0">
                <span class="block text-sm">{option.label}</span>
                {#if option.description}<span class="mt-0.5 block text-xs leading-relaxed text-muted-foreground">{option.description}</span>{/if}
              </span>
            </button>
          {/each}
          {#if row.picked.includes(OTHER)}
            <!-- svelte-ignore a11y_autofocus -->
            <input
              autofocus
              class="w-full rounded-lg border border-border bg-background px-3 py-2 text-sm outline-none focus:border-primary/50"
              placeholder="写下你的回答，回车继续"
              bind:value={row.other}
              onkeydown={(e) => e.key === "Enter" && !e.isComposing && done(row) && next()} />
          {/if}
        </div>
        <div class="mt-3 flex items-center justify-between">
          {#if step > 0}
            <button type="button" class="text-xs text-muted-foreground hover:text-foreground" onclick={() => step--}>上一题</button>
          {:else}<span></span>{/if}
          {#if row.q.multi_select || row.picked.includes(OTHER)}
            <button type="button" class="rounded-lg bg-primary px-3.5 py-1.5 text-sm text-primary-foreground disabled:opacity-40" disabled={!done(row)} onclick={next}>下一步</button>
          {/if}
        </div>
      </div>
    {:else}
      <div class="p-4">
        <div class="space-y-2">
          {#each rows as r, i (i)}
            <button type="button" class="block w-full rounded-lg px-3 py-2 text-left hover:bg-muted/60" onclick={() => (step = i)}>
              <span class="block text-xs text-muted-foreground">{r.q.header}</span>
              <span class="block text-sm">{answer(r)}</span>
            </button>
          {/each}
        </div>
        <div class="mt-3 flex items-center justify-end gap-3">
          {#if error}<span class="text-sm text-destructive">{error}</span>{/if}
          <button type="button" class="rounded-lg bg-primary px-4 py-1.5 text-sm text-primary-foreground disabled:opacity-40" disabled={sending} onclick={submit}>{sending ? "提交中…" : "提交"}</button>
        </div>
      </div>
    {/if}
  </div>
{/if}
