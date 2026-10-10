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

  /** 表格单元内 `|` 与换行会破坏行结构。 */
  const mdCell = (s: string) => s.replaceAll("\\", "\\\\").replaceAll("|", "\\|").replace(/\r?\n|\r/g, "<br>");

  async function copyMarkdown(r: QueryResult) {
    const row = (cells: string[]) => `| ${cells.map(mdCell).join(" | ")} |`;
    const lines = [row(r.columns), row(r.columns.map(() => "---")), ...r.rows.map((cells) => row(cells.map(text)))];
    try {
      await copyText(lines.join("\n"));
      copyNote = "已复制";
    } catch {
      copyNote = "复制失败，浏览器拒绝写入剪贴板";
    }
    setTimeout(() => (copyNote = null), 2000);
  }

  /** RFC 4180；NULL 为空字段；带 BOM 让 Excel 按 UTF-8 打开。 */
  function downloadCsv(r: QueryResult) {
    const field = (s: string) => (/[",\r\n]/.test(s) ? `"${s.replaceAll('"', '""')}"` : s);
    const lines = [r.columns.map(field), ...r.rows.map((cells) => cells.map((c) => (c.type === "null" ? "" : field(text(c)))))];
    const blob = new Blob(["﻿", lines.map((cells) => cells.join(",")).join("\r\n")], { type: "text/csv;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `micnext-${new Date().toISOString().slice(0, 19).replaceAll(":", "")}.csv`;
    a.click();
    URL.revokeObjectURL(url);
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
      placeholder="输入一条只读 SQL，如 SELECT * FROM core_sessions LIMIT 20"
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
        {#if result.truncated}<span class="text-amber-600 dark:text-amber-400"
            >{result.rows.length === 0
              ? "首行文本超过 1 MiB，未返回；可用 substr() 截取"
              : "结果已截断（最多 200 行 / 1 MiB）"}</span
          >{/if}
        <button
          type="button"
          class="inline-flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-accent hover:text-foreground"
          onclick={() => result && copyMarkdown(result)}><Icon name="copy" class="size-3.5" />复制为 Markdown</button>
        <button
          type="button"
          class="inline-flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-accent hover:text-foreground"
          onclick={() => result && downloadCsv(result)}><Icon name="arrowDown" class="size-3.5" />下载 CSV</button>
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
