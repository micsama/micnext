<script lang="ts">
  import { LOG_LEVELS, type LogLevel } from "../../api/developer";
  import { copyText } from "../../lib/clipboard";
  import Icon from "../../lib/Icon.svelte";
  import { loadRecent, pushRecent } from "../../lib/recent";
  import type { LogEntry, LogFeed } from "../../state/developer-logs.svelte";

  let { feed }: { feed: LogFeed } = $props();

  const PAGE = 500;
  const QUERY_MAX = 512;

  const LEVEL_CLASS: Record<LogLevel, string> = {
    debug: "text-muted-foreground",
    info: "text-sky-600 dark:text-sky-400",
    warn: "text-amber-600 dark:text-amber-400",
    error: "text-destructive",
  };

  const STATUS = { connecting: "连接中…", live: "实时", reconnecting: "已断开，正在重连…" } as const;

  /** 「全部」含之前启动的日志；切换回到底部。 */
  let scope = $state<"all" | "current">("all");
  const source = $derived.by(() => {
    void feed.version;
    return scope === "all" ? [...feed.history, ...feed.records] : feed.records;
  });

  let levels = $state<Record<LogLevel, boolean>>({ debug: true, info: true, warn: true, error: true });
  let target = $state("");
  let query = $state("");
  let regex = $state(false);
  let ignoreCase = $state(true);
  /** 最近一次有效的搜索；null = 不按文本筛选。 */
  let matcher = $state<((s: string) => boolean) | null>(null);
  let queryError = $state<string | null>(null);

  const HISTORY_KEY = "micnext.developer.logSearches";
  let history = $state(loadRecent(HISTORY_KEY));

  /** 回车或离开搜索框时记下有效搜索；边打边筛的中间态不记。 */
  function remember() {
    if (query.trim() === "") return;
    if (regex) {
      try {
        new RegExp(query, "u");
      } catch {
        return;
      }
    }
    history = pushRecent(HISTORY_KEY, history, query, 20);
  }

  $effect(() => {
    const q = query;
    const asRegex = regex;
    const flags = ignoreCase ? "ui" : "u";
    const t = setTimeout(() => {
      if (q === "") {
        matcher = null;
        queryError = null;
      } else if (!asRegex) {
        const needle = q.toLowerCase();
        matcher = (s) => s.toLowerCase().includes(needle);
        queryError = null;
      } else {
        try {
          const re = new RegExp(q, flags);
          matcher = (s) => re.test(s);
          queryError = null;
        } catch (e) {
          queryError = `正则无效，仍按上一条筛选：${(e as Error).message}`;
        }
      }
    }, 200);
    return () => clearTimeout(t);
  });

  const targets = $derived([...new Set(source.map((r) => r.target))].sort());

  const matches = $derived.by(() => {
    const m = matcher;
    return source.filter(
      (r) =>
        levels[r.level] &&
        (target === "" || r.target === target) &&
        (m === null || m(r.message) || m(r.target) || r.fields.some((f) => m(f.name) || m(f.value))),
    );
  });

  /** null = 跟随最新 PAGE 条；否则为当前页首条的 `n`，新日志与淘汰不移动视图。 */
  let anchor = $state<number | null>(null);
  const start = $derived.by(() => {
    const last = Math.max(0, matches.length - PAGE);
    if (anchor === null) return last;
    const a = anchor;
    const i = matches.findIndex((r) => r.n >= a);
    return i === -1 ? last : i;
  });
  const rows = $derived(matches.slice(start, start + PAGE));
  const following = $derived(anchor === null);
  const anchorEvicted = $derived.by(() => {
    const first = source[0];
    return anchor !== null && first !== undefined && first.n > anchor;
  });

  let list = $state<HTMLElement | null>(null);

  $effect(() => {
    void rows;
    if (following && list) list.scrollTop = list.scrollHeight;
  });

  function pause() {
    anchor = matches[start]?.n ?? feed.next;
  }

  function follow() {
    anchor = null;
  }

  function onscroll() {
    if (!following || !list) return;
    if (list.scrollHeight - list.scrollTop - list.clientHeight > 8) pause();
  }

  function setScope(next: "all" | "current") {
    scope = next;
    anchor = null;
  }

  /** 与上一行属于不同启动时返回分隔文案。 */
  function bootLabel(i: number): string | null {
    const g = start + i;
    const boot = matches[g]!.boot_id;
    if (g > 0 && matches[g - 1]!.boot_id === boot) return null;
    return boot === null ? "本次启动" : `启动于 ${new Date(boot).toLocaleString()}`;
  }

  const storageNote = $derived.by(() => {
    const s = feed.storage;
    if (s === null || s.kind === "sqlite") return null;
    return s.kind === "file"
      ? `日志库不可用（${s.reason}），已改写到 ${s.path}`
      : `日志只在内存（${s.reason}），重启后不可回看`;
  });

  let copyNote = $state<string | null>(null);

  async function copyAll() {
    try {
      await copyText(matches.map(line).join("\n"));
      copyNote = `已复制 ${matches.length} 条`;
    } catch {
      copyNote = "复制失败，浏览器拒绝写入剪贴板";
    }
    setTimeout(() => (copyNote = null), 2000);
  }

  function time(ms: number): string {
    const d = new Date(ms);
    const pad = (n: number, w = 2) => String(n).padStart(w, "0");
    return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
  }

  function line(r: LogEntry): string {
    const fields = r.fields.map((f) => ` ${f.name}=${f.value}`).join("");
    const at = r.file ? ` (${r.file}:${r.line ?? "?"})` : "";
    return `${new Date(r.timestamp_ms).toISOString()} ${r.level.toUpperCase()} ${r.target} ${r.message}${fields}${at}${r.truncated ? " [已截断]" : ""}`;
  }
</script>

<div class="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 border-b px-3 py-2 text-xs">
  <div class="flex overflow-hidden rounded-md border">
    {#each [["all", "全部"], ["current", "本次启动以来"]] as const as [value, label] (value)}
      <button
        type="button"
        class="px-2 py-1 {scope === value ? 'bg-accent text-foreground' : 'text-muted-foreground hover:bg-accent/50'}"
        onclick={() => setScope(value)}>{label}</button>
    {/each}
  </div>
  <div class="flex items-center gap-2">
    {#each LOG_LEVELS as l (l)}
      <label class="flex items-center gap-1">
        <input type="checkbox" bind:checked={levels[l]} />
        <span class={LEVEL_CLASS[l]}>{l.toUpperCase()}</span>
      </label>
    {/each}
  </div>
  <select bind:value={target} class="max-w-60 rounded-md border bg-background px-2 py-1">
    <option value="">全部模块</option>
    {#each targets as t (t)}
      <option value={t}>{t}</option>
    {/each}
  </select>
  <input
    bind:value={query}
    list="developer-log-searches"
    onkeydown={(e) => e.key === "Enter" && remember()}
    onblur={remember}
    maxlength={QUERY_MAX}
    placeholder={regex ? "正则（搜索正文、模块、字段）" : "搜索正文、模块、字段"}
    class="min-w-48 flex-1 rounded-md border bg-background px-2 py-1 font-mono" />
  <datalist id="developer-log-searches">
    {#each history as h (h)}<option value={h}></option>{/each}
  </datalist>
  <label class="flex items-center gap-1"><input type="checkbox" bind:checked={regex} />正则</label>
  <label class="flex items-center gap-1" class:opacity-50={!regex}>
    <input type="checkbox" bind:checked={ignoreCase} disabled={!regex} />忽略大小写
  </label>
</div>
{#if storageNote}
  <p class="shrink-0 border-b px-3 py-1 text-xs text-amber-600 dark:text-amber-400">{storageNote}</p>
{/if}
{#if feed.historyError}
  <p class="shrink-0 border-b px-3 py-1 text-xs text-destructive">{feed.historyError}</p>
{/if}
{#if queryError}
  <p class="shrink-0 border-b px-3 py-1 text-xs text-destructive">{queryError}</p>
{/if}
{#if anchorEvicted}
  <p class="shrink-0 border-b px-3 py-1 text-xs text-amber-600 dark:text-amber-400">
    暂停处的日志已被淘汰，当前从最早保留的记录显示
  </p>
{/if}

<div bind:this={list} {onscroll} class="min-h-0 flex-1 overflow-y-auto font-mono text-xs">
  {#each rows as r, i (i)}
    {@const boot = scope === "all" ? bootLabel(i) : null}
    {#if boot}
      <div class="border-b border-border/50 px-3 py-0.5 text-center text-muted-foreground">—— {boot} ——</div>
    {/if}
    <div class="border-b border-border/50 px-3 py-0.5 break-all whitespace-pre-wrap">
      <span class="text-muted-foreground">{time(r.timestamp_ms)}</span>
      <span class={LEVEL_CLASS[r.level]}>{r.level.toUpperCase().padEnd(5)}</span>
      <span class="text-muted-foreground" title={r.file ? `${r.file}:${r.line ?? "?"}` : undefined}>{r.target}</span>
      {r.message}{#each r.fields as f, j (j)}<span class="text-muted-foreground"> {f.name}=</span>{f.value}{/each}{#if r.truncated}<span
          class="text-amber-600 dark:text-amber-400"> [已截断]</span>{/if}
    </div>
  {:else}
    <p class="p-3 text-muted-foreground">{source.length === 0 ? "暂无日志" : "没有匹配的日志"}</p>
  {/each}
</div>

<div class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 border-t px-3 py-1.5 text-xs text-muted-foreground">
  {#if feed.fatal}
    <span class="text-destructive">{feed.fatal}</span>
    <button type="button" class="rounded px-1.5 py-0.5 hover:bg-accent hover:text-foreground" onclick={() => feed.reconnect()}
      >重新连接</button>
  {:else}
    <span class:text-destructive={feed.status === "reconnecting"}>{STATUS[feed.status]}</span>
  {/if}
  <span>{matches.length} / {source.length} 条{feed.historyTrimmed ? "（更早的已淘汰）" : ""}</span>
  <span class="flex items-center gap-1">
    <button
      type="button"
      class="rounded p-0.5 hover:bg-accent disabled:opacity-40"
      aria-label="上一页"
      disabled={start === 0}
      onclick={() => (anchor = matches[Math.max(0, start - PAGE)]!.n)}
      ><Icon name="chevron" class="size-3.5 rotate-180" /></button>
    第 {rows.length === 0 ? 0 : start + 1}–{start + rows.length} 条
    <button
      type="button"
      class="rounded p-0.5 hover:bg-accent disabled:opacity-40"
      aria-label="下一页"
      disabled={start + PAGE >= matches.length}
      onclick={() => (anchor = matches[start + PAGE]!.n)}><Icon name="chevron" class="size-3.5" /></button>
  </span>
  {#if following}
    <button type="button" class="rounded px-1.5 py-0.5 hover:bg-accent hover:text-foreground" onclick={pause}>暂停滚动</button>
  {:else}
    <button type="button" class="rounded px-1.5 py-0.5 hover:bg-accent hover:text-foreground" onclick={follow}
      >回到底部</button>
  {/if}
  <button
    type="button"
    class="inline-flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-accent hover:text-foreground"
    onclick={copyAll}><Icon name="copy" class="size-3.5" />复制筛选结果</button>
  {#if copyNote}<span>{copyNote}</span>{/if}
  <span class="ml-auto">保留最近日志（最多 100 MB）；「全部」含之前启动的最近 2000 条</span>
</div>
