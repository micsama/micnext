<script lang="ts">
  import hljs from "highlight.js/lib/common";
  import { onMount } from "svelte";
  import { getSqlSchema, reviewSql, runSql } from "../../api/client";
  import type { Cell, QueryResult, SqlReview, SqlRisk, TableSchema } from "../../api/developer";
  import { copyText } from "../../lib/clipboard";
  import Icon from "../../lib/Icon.svelte";
  import { loadRecent, pushRecent } from "../../lib/recent";

  let sql = $state("");
  let running = $state(false);
  let result = $state.raw<QueryResult | null>(null);
  let error = $state<string | null>(null);
  let tables = $state.raw<TableSchema[] | null>(null);
  const groups = $derived([...new Set(tables?.map((t) => t.group))]);
  let schemaError = $state<string | null>(null);
  let copyNote = $state<string | null>(null);

  /** 写入模式不记住，刷新后回到只读。 */
  let write = $state(false);
  let review = $state.raw<{ sql: string; review: SqlReview } | null>(null);
  /** 改了 SQL 后旧的把关结果作废。 */
  const pending = $derived(write && review?.sql === sql ? review.review : null);
  const RISK: Record<SqlRisk, { label: string; tone: string }> = {
    low: { label: "低", tone: "border-emerald-500/60 bg-emerald-500/5" },
    medium: { label: "中", tone: "border-amber-500/70 bg-amber-500/10" },
    high: { label: "高", tone: "border-destructive bg-destructive/10" },
  };

  const RECENT_KEY = "micnext.developer.sqlRecent";
  let recent = $state(loadRecent(RECENT_KEY));

  /** 末尾补换行，让最后一个空行在高亮层里也占高度。 */
  const highlighted = $derived(hljs.highlight(sql, { language: "sql", ignoreIllegals: true }).value + "\n");
  let backdrop = $state<HTMLElement | null>(null);

  const HEIGHT_KEY = "micnext.developer.sqlEditorHeight";
  const HEIGHT_MIN = 80;
  /** 输入框高度，拖动分隔条调整，按浏览器记住。 */
  let editorHeight = $state(loadHeight());
  let pane = $state<HTMLElement | null>(null);

  function loadHeight(): number {
    try {
      const v = Number(localStorage.getItem(HEIGHT_KEY));
      return v >= HEIGHT_MIN ? v : 200;
    } catch {
      return 200;
    }
  }

  function startResize(e: PointerEvent) {
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const y0 = e.clientY;
    const h0 = editorHeight;
    const max = Math.max(HEIGHT_MIN, (pane?.clientHeight ?? 600) - 120);
    const move = (ev: PointerEvent) => {
      editorHeight = Math.min(max, Math.max(HEIGHT_MIN, h0 + ev.clientY - y0));
    };
    const up = () => {
      handle.removeEventListener("pointermove", move);
      try {
        localStorage.setItem(HEIGHT_KEY, String(editorHeight));
      } catch {
        // NOTE: 存储不可用时仅本次有效。
      }
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up, { once: true });
  }

  onMount(() => {
    getSqlSchema().then(
      (t) => (tables = t),
      (e: unknown) => (schemaError = (e as Error).message),
    );
  });

  /** 写入模式下先把关，确认后才执行。 */
  async function run(confirmed = false) {
    if (running || sql.trim() === "") return;
    running = true;
    error = null;
    const q = sql;
    try {
      if (write && !confirmed) {
        review = null;
        review = { sql: q, review: await reviewSql(q) };
        return;
      }
      review = null;
      result = await runSql(q, write);
      recent = pushRecent(RECENT_KEY, recent, q.trim(), 30);
    } catch (e) {
      if (confirmed) result = null;
      error = (e as Error).message;
    } finally {
      running = false;
    }
  }

  function toggleWrite() {
    write = !write;
    review = null;
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
    {#if recent.length > 0}
      <details open class="mb-2 border-b pb-2">
        <summary class="cursor-pointer text-muted-foreground">最近</summary>
        <!-- 点选只填回输入框，不自动执行。 -->
        {#each recent as q (q)}
          <button
            type="button"
            title={q}
            class="block w-full truncate rounded px-1 py-0.5 text-left font-mono hover:bg-accent"
            onclick={() => (sql = q)}>{q.split("\n", 1)[0]}</button>
        {/each}
      </details>
    {/if}
    {#if schemaError}
      <p class="text-destructive">{schemaError}</p>
    {:else if !tables}
      <p class="text-muted-foreground">加载中…</p>
    {:else}
      {#each groups as group (group)}
        <details open class="mb-2">
          <summary class="cursor-pointer font-medium">{group}</summary>
      {#each tables.filter((t) => t.group === group) as t (t.name)}
        <details class="py-0.5">
          <summary class="cursor-pointer font-mono" title={t.description}>{t.name}</summary>
          <p class="pl-4 text-muted-foreground">{t.description}</p>
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
        </details>
      {/each}
    {/if}
  </aside>

  <div bind:this={pane} class="flex min-h-0 min-w-0 flex-1 flex-col">
    <!-- 透明文字的 textarea 叠在高亮层上，两层字体、内边距、换行与滚动须一致。 -->
    {#if write}
      <p class="shrink-0 border-b border-destructive bg-destructive/10 px-3 py-2 text-xs font-medium text-destructive">
        写入模式：每次执行前先备份整个数据库，再直接改主库。改动立即生效，已打开的页面不会自动刷新，部分设置可能要重启才生效。改错时，停服务后用备份文件替换 micnext.db。
      </p>
    {/if}
    <div class={["relative shrink-0 bg-background font-mono text-sm", write && "border-4 border-destructive"]} style:height="{editorHeight}px">
      <pre
        bind:this={backdrop}
        aria-hidden="true"
        class="pointer-events-none absolute inset-0 m-0 overflow-y-scroll p-3 break-words whitespace-pre-wrap"><code
          >{@html highlighted}</code></pre>
      <textarea
        bind:value={sql}
        {onkeydown}
        onscroll={(e) => backdrop && (backdrop.scrollTop = e.currentTarget.scrollTop)}
        spellcheck="false"
        placeholder={write ? "输入一条 SQL，可增删改，如 UPDATE core_sessions SET … WHERE …" : "输入一条只读 SQL，如 SELECT * FROM core_sessions LIMIT 20"}
        class="absolute inset-0 resize-none overflow-y-scroll bg-transparent p-3 break-words whitespace-pre-wrap text-transparent caret-foreground outline-none placeholder:text-muted-foreground"
      ></textarea>
    </div>
    <div
      role="separator"
      aria-orientation="horizontal"
      aria-label="拖动调整输入框高度"
      class="h-1.5 shrink-0 cursor-row-resize touch-none border-y bg-muted hover:bg-accent"
      onpointerdown={startResize}></div>
    <div class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 border-b px-3 py-1.5 text-xs text-muted-foreground">
      <button
        type="button"
        disabled={running || sql.trim() === ""}
        class={["inline-flex items-center gap-1 rounded-md px-3 py-1 disabled:opacity-50", write ? "bg-destructive text-white" : "bg-primary text-primary-foreground"]}
        onclick={() => run()}>
        {#if running}<Icon name="loader" class="size-3.5 animate-spin" />{/if}{write ? "把关" : "执行"}
      </button>
      <span>Ctrl/⌘ + Enter</span>
      <button
        type="button"
        role="switch"
        aria-checked={write}
        class={["inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5", write ? "border-destructive text-destructive" : "hover:bg-accent hover:text-foreground"]}
        onclick={toggleWrite}>
        <span class={["relative h-3.5 w-6 rounded-full transition-colors", write ? "bg-destructive" : "bg-muted-foreground/40"]}
          ><span class={["absolute top-0.5 size-2.5 rounded-full bg-white transition-all", write ? "left-3" : "left-0.5"]}></span></span
        >写入模式
      </button>
      {#if result}
        <span>{result.rows.length} 行 · {result.elapsed_ms} ms</span>
        {#if result.changes !== null}<span class="text-destructive">影响 {result.changes} 行 · 已备份 {result.backup}</span>{/if}
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
      <span class="ml-auto">{write ? "主库；先试运行与 AI 把关，确认后备份再执行；凭据列读作 NULL 且不可写" : "只读连接；凭据列读作 NULL"}</span>
    </div>
    {#if pending}
      {@const risk = pending.risk ? RISK[pending.risk] : null}
      <div class={["shrink-0 space-y-1.5 border-y-2 px-3 py-2 text-sm", risk?.tone ?? "border-border bg-muted/50"]}>
        <p class="font-medium">
          {#if risk}风险：{risk.label}{:else}未经 AI 评估{/if}
          <span class="ml-2 text-xs font-normal text-muted-foreground"
            >试运行影响 {pending.changes} 行{#if pending.tables.length > 0} · 涉及 {pending.tables.join("、")}{/if}</span>
        </p>
        {#if pending.reason}<p class="whitespace-pre-wrap">{pending.reason}</p>{/if}
        {#if pending.ai_error}<p class="text-xs text-muted-foreground">{pending.ai_error}</p>{/if}
        <div class="flex gap-2">
          <button
            type="button"
            disabled={running}
            class="inline-flex items-center gap-1 rounded-md bg-destructive px-3 py-1 text-xs text-white disabled:opacity-50"
            onclick={() => run(true)}>
            {#if running}<Icon name="loader" class="size-3.5 animate-spin" />{/if}确认执行
          </button>
          <button type="button" class="rounded-md px-3 py-1 text-xs hover:bg-accent" onclick={() => (review = null)}>取消</button>
        </div>
      </div>
    {/if}
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
