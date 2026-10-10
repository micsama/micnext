<script lang="ts">
  import { onMount } from "svelte";
  import { getSqlSchema, runSql } from "../../api/client";
  import type { Cell, QueryResult, TableSchema } from "../../api/developer";
  import { copyText } from "../../lib/clipboard";
  import Icon from "../../lib/Icon.svelte";

  let sql = $state("");
  let running = $state(false);
  let result = $state.raw<QueryResult | null>(null);
  let error = $state<string | null>(null);
  let tables = $state.raw<TableSchema[] | null>(null);
  let schemaError = $state<string | null>(null);
  let copyNote = $state<string | null>(null);

  onMount(() => {
    getSqlSchema().then(
      (t) => (tables = t),
      (e: unknown) => (schemaError = (e as Error).message),
    );
  });

  async function run() {
    if (running || sql.trim() === "") return;
    running = true;
    error = null;
    try {
      result = await runSql(sql);
    } catch (e) {
      result = null;
      error = (e as Error).message;
    } finally {
      running = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void run();
    }
  }

  function text(c: Cell): string {
    switch (c.type) {
      case "null":
        return "NULL";
      case "blob":
        return `<BLOB ${c.bytes} 字节>`;
      default:
        return String(c.value);
    }
  }

  const escape = (s: string) =>
    s.replaceAll("\\", "\\\\").replaceAll("\t", "\\t").replaceAll("\n", "\\n").replaceAll("\r", "\\r");

  async function copyTsv(r: QueryResult) {
    const lines = [r.columns, ...r.rows.map((row) => row.map(text))].map((cells) => cells.map(escape).join("\t"));
    try {
      await copyText(lines.join("\n"));
      copyNote = "已复制";
    } catch {
      copyNote = "复制失败，浏览器拒绝写入剪贴板";
    }
    setTimeout(() => (copyNote = null), 2000);
  }
</script>

<div class="flex min-h-0 flex-1 flex-col md:flex-row">
  <aside class="shrink-0 overflow-y-auto border-b p-3 text-xs max-md:max-h-40 md:w-60 md:border-r md:border-b-0">
    {#if schemaError}
      <p class="text-destructive">{schemaError}</p>
    {:else if !tables}
      <p class="text-muted-foreground">加载中…</p>
    {:else}
      {#each tables as t (t.name)}
        <details class="py-0.5">
          <summary class="cursor-pointer font-mono">{t.name}</summary>
          <ul class="pb-1 pl-4">
            {#each t.columns as c (c.name)}
              <li class="font-mono">
                {c.name} <span class="text-muted-foreground">{c.decl_type}</span>
                {#if c.secret}<span class="rounded bg-muted px-1 text-muted-foreground">已隐藏</span>{/if}
              </li>
            {/each}
          </ul>
        </details>
      {/each}
    {/if}
  </aside>

  <div class="flex min-h-0 min-w-0 flex-1 flex-col">
    <textarea
      bind:value={sql}
      {onkeydown}
      spellcheck="false"
      placeholder="输入一条只读 SQL，如 SELECT * FROM session LIMIT 20"
      class="h-32 shrink-0 resize-y border-b bg-background p-3 font-mono text-sm outline-none"></textarea>
    <div class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 border-b px-3 py-1.5 text-xs text-muted-foreground">
      <button
        type="button"
        disabled={running || sql.trim() === ""}
        class="inline-flex items-center gap-1 rounded-md bg-primary px-3 py-1 text-primary-foreground disabled:opacity-50"
        onclick={run}>
        {#if running}<Icon name="loader" class="size-3.5 animate-spin" />{/if}执行
      </button>
      <span>Ctrl/⌘ + Enter</span>
      {#if result}
        <span>{result.rows.length} 行 · {result.elapsed_ms} ms</span>
        {#if result.truncated}<span class="text-amber-600 dark:text-amber-400">结果已截断（最多 200 行 / 1 MiB）</span>{/if}
        <button
          type="button"
          class="inline-flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-accent hover:text-foreground"
          onclick={() => result && copyTsv(result)}><Icon name="copy" class="size-3.5" />复制为 TSV</button>
        {#if copyNote}<span>{copyNote}</span>{/if}
      {/if}
      <span class="ml-auto">只读连接；凭据列读作 NULL</span>
    </div>
    {#if error}
      <p class="shrink-0 border-b px-3 py-2 text-sm text-destructive">{error}</p>
    {/if}
    <div class="min-h-0 flex-1 overflow-auto">
      {#if result}
        <table class="min-w-full border-collapse font-mono text-xs">
          <thead class="sticky top-0 bg-muted">
            <tr>
              {#each result.columns as c, i (i)}
                <th class="border-b px-3 py-1.5 text-left font-medium whitespace-nowrap">{c}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each result.rows as row, i (i)}
              <tr class="border-b border-border/50 align-top">
                {#each row as c, j (j)}
                  <td
                    class="max-w-md px-3 py-1 break-all whitespace-pre-wrap"
                    class:text-muted-foreground={c.type === "null" || c.type === "blob"}
                    class:italic={c.type === "null"}>{text(c)}</td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
        {#if result.rows.length === 0}
          <p class="p-3 text-xs text-muted-foreground">无结果</p>
        {/if}
      {/if}
    </div>
  </div>
</div>
