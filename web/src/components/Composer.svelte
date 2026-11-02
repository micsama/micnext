<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import { listCommands, listSkills } from "../api/client";
  import type { CommandSpec, SkillList } from "../api/types";
  import type { InputPart } from "../api/types";
  import { fileIconUrl } from "../lib/file-icon.svelte";
  import { formatBytes } from "../lib/format";
  import { theme } from "../state/theme.svelte";
  import Icon from "../lib/Icon.svelte";
  import { settingsStore } from "../state/settings.svelte";
  import type { ComposerReply } from "../state/session.svelte";

  let {
    onsend,
    placeholder = "输入消息，回车发送，Shift / Option + 回车换行",
    autofocus = false,
    leading,
    trailing,
    notice = null,
    dock,
    readonly = null,
    blocked = false,
    danger = false,
    onstop,
    stopping = false,
    reply = $bindable(null),
    prediction = null,
  }: {
    onsend: (parts: InputPart[]) => Promise<void>;
    placeholder?: string;
    autofocus?: boolean;
    /** 工具栏左侧、附件按钮之后的本会话选择（如人设）。 */
    leading?: Snippet;
    /** 工具栏右侧、发送按钮之前的本会话选择（如模型）。 */
    trailing?: Snippet;
    /** 输入框上方的提示。 */
    notice?: string | null;
    /** 贴在输入框上沿的内容（如后台任务条）。 */
    dock?: Snippet;
    /** 不可输入时的说明，替代占位文字；附件与发送随之隐藏。 */
    readonly?: string | null;
    /** 暂不能发送（原因见 notice）。 */
    blocked?: boolean;
    /** 红框提醒（如临时明文开启中）。 */
    danger?: boolean;
    /** 有一轮在执行时传入：输入为空时发送键变为停止，Esc 同效。 */
    onstop?: () => Promise<void>;
    stopping?: boolean;
    /** 回复引用；发送成功后清空。 */
    reply?: ComposerReply | null;
    /** 预测的下一句：输入框为空时代替占位文字，Tab 或「填入」采用。 */
    prediction?: string | null;
  } = $props();

  const IMAGE_TYPES = ["image/png", "image/jpeg", "image/webp", "image/gif"];

  /** 支持的图片带预览 `url`，按图片发送；其余按文件发送。 */
  type Attachment = { id: number; file: File; url: string | null };
  let nextId = 0;

  let text = $state("");
  /** `/` 补全项：内置指令与 skill 同列；不合法的 skill 带原因，不可选。 */
  type Suggestion = { key: string; usage: string; description: string; skill: boolean; invalid: string | null };
  let commands = $state<CommandSpec[]>([]);
  let skills = $state<SkillList>({ skills: [], invalid: [] });
  let commandIndex = $state(0);
  onMount(() => { void listCommands().then((items) => { commands = items; }).catch((e: Error) => { error = e.message; }); });
  function complete(s: Suggestion) { if (s.invalid !== null || !picking) return; text = `${s.usage.split(" ")[0]} `; commandIndex = 0; input.focus(); }

  let attachments = $state<Attachment[]>([]);
  const slash = $derived(text.trim().startsWith("/") && attachments.length === 0);
  /** 输入的指令名；后面跟了参数时补全框只留匹配项作提示，不再响应选择键。 */
  const head = $derived(text.trim().split(/\s/, 1)[0]!);
  const picking = $derived(head === text.trim());
  // skill 可能随时被增删，每次打开补全时重新拉取。
  $effect(() => { if (slash) void listSkills().then((list) => { skills = list; }).catch((e: Error) => { error = e.message; }); });
  const entries = $derived<Suggestion[]>([
    ...commands.map((c) => ({ key: c.name, usage: c.usage, description: c.description, skill: false, invalid: null })),
    ...skills.skills.map((s) => ({ key: s.name, usage: `/${s.name}`, description: s.description, skill: true, invalid: null })),
    ...skills.invalid.map((s) => ({ key: s.dir, usage: `/${s.dir}`, description: "不可用", skill: true, invalid: s.reason })),
  ]);
  const suggestions = $derived(!slash ? [] : picking ? entries.filter((e) => e.usage.startsWith(head)) : entries.filter((e) => e.usage.split(" ")[0] === head));
  let picker: HTMLInputElement;
  let dragging = $state(false);
  let dragDepth = 0;
  const limits = $derived(settingsStore.inputLimits);
  let posting = $state(false);
  const acceptsFiles = $derived(!!limits && readonly === null && !posting);
  let error = $state<string | null>(null);
  let input: HTMLTextAreaElement;
  const MAX_HEIGHT_PX = 240;

  const guess = $derived(readonly === null && text === "" && attachments.length === 0 ? prediction : null);
  const coarse = window.matchMedia("(pointer: coarse)").matches;
  function accept() {
    if (guess === null) return;
    text = guess;
    input.focus();
  }

  const canSend = $derived((text.trim() !== "" || attachments.length > 0) && !posting && !blocked);
  const stopMode = $derived(!!onstop && text.trim() === "" && attachments.length === 0 && !posting);

  async function stop() {
    if (!onstop || stopping) return;
    error = null;
    try { await onstop(); }
    catch (e) { error = (e as Error).message; }
  }

  $effect(() => {
    void text;
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, MAX_HEIGHT_PX)}px`;
  });
  $effect(() => {
    if (autofocus || reply) input.focus();
  });

  const release = (a: Attachment) => a.url && URL.revokeObjectURL(a.url);
  $effect(() => () => attachments.forEach(release));

  function attach(files: File[]) {
    if (!limits || !acceptsFiles || files.length === 0) return;
    error = null;
    for (const f of files) {
      const image = IMAGE_TYPES.includes(f.type);
      const count = attachments.filter((a) => (a.url !== null) === image).length;
      if (image && f.size > limits.max_image_bytes) {
        error = `图片不能超过 ${limits.max_image_bytes / 1024 / 1024} MB`;
      } else if (image && count >= limits.max_images) {
        error = `一条消息最多带 ${limits.max_images} 张图片`;
      } else if (!image && f.size > limits.max_file_bytes) {
        error = `文件不能超过 ${limits.max_file_bytes / 1024 / 1024} MB`;
      } else if (!image && count >= limits.max_files) {
        error = `一条消息最多带 ${limits.max_files} 个文件`;
      } else {
        attachments.push({ id: nextId++, file: f, url: image ? URL.createObjectURL(f) : null });
      }
    }
    input.focus();
  }

  function detach(i: number) {
    release(attachments[i]!);
    attachments.splice(i, 1);
  }

  async function toBase64(file: File): Promise<string> {
    const bytes = new Uint8Array(await file.arrayBuffer());
    let bin = "";
    for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(bin);
  }

  function onpaste(e: ClipboardEvent) {
    const files = [...(e.clipboardData?.files ?? [])];
    if (files.length > 0) {
      e.preventDefault();
      attach(files);
    }
  }

  function ondrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    dragDepth = 0;
    attach([...(e.dataTransfer?.files ?? [])]);
  }

  onMount(() => {
    const area = input.closest("main");
    if (!area) return;
    const isFile = (e: DragEvent) => e.dataTransfer?.types.includes("Files");
    const enter = (e: DragEvent) => {
      if (!isFile(e)) return;
      e.preventDefault();
      dragDepth += 1;
      dragging = acceptsFiles;
    };
    const over = (e: DragEvent) => {
      if (!isFile(e)) return;
      e.preventDefault();
      e.dataTransfer!.dropEffect = acceptsFiles ? "copy" : "none";
      dragging = acceptsFiles;
    };
    const leave = (e: DragEvent) => {
      if (!isFile(e)) return;
      dragDepth = Math.max(0, dragDepth - 1);
      if (dragDepth === 0 || !e.relatedTarget || (e.relatedTarget instanceof Node && !area.contains(e.relatedTarget))) reset();
    };
    const reset = () => { dragging = false; dragDepth = 0; };
    const drop = (e: DragEvent) => {
      if (!isFile(e)) return;
      ondrop(e);
    };
    area.addEventListener("dragenter", enter);
    area.addEventListener("dragover", over);
    area.addEventListener("dragleave", leave);
    area.addEventListener("drop", drop);
    window.addEventListener("dragend", reset);
    window.addEventListener("blur", reset);
    return () => {
      area.removeEventListener("dragenter", enter);
      area.removeEventListener("dragover", over);
      area.removeEventListener("dragleave", leave);
      area.removeEventListener("drop", drop);
      window.removeEventListener("dragend", reset);
      window.removeEventListener("blur", reset);
    };
  });

  async function send() {
    if (!canSend) return;
    posting = true;
    error = null;
    try {
      const parts: InputPart[] = [];
      if (text.trim() !== "") parts.push({ kind: "text", text: reply ? `${reply.quote.replace(/^/gm, "> ")}\n\n${text}` : text });
      for (const a of attachments) {
        const base64 = await toBase64(a.file);
        parts.push(a.url ? { kind: "image", base64 } : { kind: "file", name: a.file.name, base64 });
      }
      await onsend(parts);
      attachments.forEach(release);
      attachments = [];
      text = "";
      reply = null;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      posting = false;
      queueMicrotask(() => input?.focus());
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (picking && suggestions.length > 0 && !e.isComposing) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        commandIndex = (commandIndex + (e.key === "ArrowDown" ? 1 : -1) + suggestions.length) % suggestions.length;
        return;
      }
      if (e.key === "Tab") { e.preventDefault(); complete(suggestions[commandIndex % suggestions.length]!); return; }
    }
    if ((e.key === "Tab" || e.key === "ArrowRight") && guess !== null && !e.isComposing && !e.shiftKey) {
      e.preventDefault();
      accept();
      return;
    }
    if (e.key === "Escape" && stopMode) {
      e.preventDefault();
      void stop();
      return;
    }
    // 输入法选词中的回车不处理。
    if (e.key !== "Enter" || e.isComposing || e.keyCode === 229) return;
    if (e.metaKey || e.ctrlKey) {
      e.preventDefault();
      void send();
    } else if (e.shiftKey || e.altKey || listItem()) {
      e.preventDefault();
      newline();
    } else {
      e.preventDefault();
      void send();
    }
  }

  /** 列表项行首：缩进、无序标记或带分隔符的序号、空白。 */
  const LIST_ITEM = /^(\s*)(?:([-*+•])|(\d+)([.)、]))(\s+)/;

  /** 光标所在行是列表项且光标在标记之后时，返回该行起点与标记。 */
  function listItem(): { start: number; marker: RegExpExecArray } | null {
    const caret = input.selectionStart;
    if (caret !== input.selectionEnd) return null;
    const start = text.lastIndexOf("\n", caret - 1) + 1;
    const marker = LIST_ITEM.exec(text.slice(start, caret));
    return marker ? { start, marker } : null;
  }

  /** 换行；在列表项里续上下一个标记，空列表项则退出列表。用 execCommand 以保留撤销。 */
  function newline() {
    const item = listItem();
    if (!item) {
      document.execCommand("insertText", false, "\n");
      return;
    }
    const [whole, indent, bullet, num, delim, space] = item.marker;
    const end = text.indexOf("\n", item.start);
    if (text.slice(item.start + whole.length, end === -1 ? text.length : end).trim() === "") {
      input.setSelectionRange(item.start, item.start + whole.length);
      document.execCommand("delete");
      return;
    }
    const next = bullet ?? `${Number(num) + 1}${delim}`;
    document.execCommand("insertText", false, `\n${indent}${next}${space}`);
  }
</script>

<div class="mx-auto w-full max-w-chat px-4 pb-4">
  {#if error}
    <p class="mb-2 text-sm text-destructive">{error}</p>
  {:else if notice}
    <p class="mb-2 text-sm text-warning">{notice}</p>
  {/if}
  {@render dock?.()}
  <div
    role="group"
    data-composer-input
    class="relative z-20 rounded-3xl border bg-card p-2 shadow-sm transition-colors {dragging
      ? 'border-primary'
      : danger
        ? 'border-destructive'
        : 'border-border focus-within:border-ring'}">
    {#if dragging}
      <div class="pointer-events-none absolute inset-1 z-30 flex items-center justify-center gap-3 rounded-[20px] border border-dashed border-primary/50 bg-card/95 px-4 text-primary">
        <span class="grid size-10 shrink-0 place-items-center rounded-xl bg-primary/10"><Icon name="download" class="size-5" /></span>
        <div>
          <p class="text-sm font-medium">松开以添加附件</p>
          <p class="mt-0.5 text-xs text-muted-foreground">图片或文件 · 添加后可继续输入消息</p>
        </div>
      </div>
    {/if}
    {#if attachments.length > 0}
      <div class="flex gap-2 overflow-x-auto px-1 pt-1 pb-2" aria-label="待发送附件">
        {#each attachments as a, i (a.id)}
          <div class="flex h-[72px] w-56 shrink-0 items-center gap-2.5 rounded-2xl border border-border/70 bg-muted/40 p-2">
            {#if a.url}
              <img src={a.url} alt={a.file.name} class="size-12 shrink-0 rounded-lg object-cover" />
            {:else}
              <span class="grid size-12 shrink-0 place-items-center rounded-lg bg-background">
                <img src={fileIconUrl(a.file.name, theme.dark)} alt="" class="size-7" />
              </span>
            {/if}
            <div class="min-w-0 flex-1">
              <p class="truncate text-xs font-medium" title={a.file.name}>{a.file.name}</p>
              <p class="mt-1 text-xs text-muted-foreground tabular-nums">{formatBytes(a.file.size)}</p>
            </div>
            <button
              type="button"
              onclick={() => detach(i)}
              disabled={posting}
              aria-label={`移除 ${a.file.name}`}
              title="移除附件"
              class="grid size-7 shrink-0 place-items-center rounded-full text-muted-foreground transition-colors hover:bg-background hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-30">
              <Icon name="x" class="size-3.5" />
            </button>
          </div>
        {/each}
      </div>
    {/if}
    {#if reply}
      <div class="mx-2 mt-1 flex items-center gap-1.5 rounded-lg bg-muted py-1 pr-1 pl-2 text-xs text-muted-foreground">
        <Icon name="reply" class="size-3.5 shrink-0" />
        <span class="min-w-0 flex-1 truncate" title={reply.quote}>{reply.label}</span>
        <button type="button" aria-label="取消回复" title="取消回复" disabled={posting} class="grid size-5 shrink-0 place-items-center rounded hover:bg-accent hover:text-foreground" onclick={() => (reply = null)}><Icon name="x" class="size-3" /></button>
      </div>
    {/if}
    {#if suggestions.length > 0}
      <div class="mb-1 rounded-xl border border-border bg-background p-1">
        {#each suggestions as s, i (`${s.skill}/${s.key}`)}
          <button type="button" class={["flex w-full items-center justify-between gap-3 rounded-lg px-3 py-2 text-left text-sm", s.invalid === null ? "hover:bg-accent" : "cursor-not-allowed opacity-50", i === commandIndex % suggestions.length && "bg-accent"]} title={s.invalid ?? undefined} onclick={() => complete(s)}>
            <span class="flex shrink-0 items-center gap-1.5 font-mono">{#if s.skill}<Icon name="zap" class="size-3.5 text-muted-foreground" />{/if}{s.usage}</span><span class="min-w-0 truncate text-xs text-muted-foreground" title={s.invalid ?? s.description}>{s.description}</span>
          </button>
        {/each}
      </div>
    {/if}
    <div class="relative">
      <textarea
        bind:this={input}
        bind:value={text}
        {onkeydown}
        {onpaste}
        placeholder={readonly ?? (guess !== null ? `✦ ${guess}` : placeholder)}
        disabled={posting || readonly !== null}
        rows="1"
        class="block max-h-60 min-h-12 w-full resize-none bg-transparent px-3 pt-2 pb-1 text-base outline-none placeholder:text-muted-foreground {posting ? 'opacity-60' : ''} {guess !== null && coarse ? 'pr-14' : ''}"
      ></textarea>
      {#if guess !== null && coarse}
        <button type="button" onclick={accept} class="absolute top-2 right-2 rounded-full bg-muted px-2.5 py-1 text-xs text-foreground hover:bg-accent">填入</button>
      {/if}
    </div>
    <div class="flex items-center gap-1">
      <input
        bind:this={picker}
        type="file"
        multiple
        hidden
        onchange={() => {
          attach([...(picker.files ?? [])]);
          picker.value = "";
        }} />
      {#if limits && readonly === null}
        <button
          type="button"
          onclick={() => picker.click()}
          disabled={posting}
          aria-label="添加图片或文件"
          title="添加图片或文件"
          class="grid size-9 shrink-0 place-items-center rounded-full bg-muted text-foreground hover:bg-accent disabled:opacity-30">
          <Icon name="plus" class="size-5" />
        </button>
      {/if}
      {@render leading?.()}
      <div class="flex-1"></div>
      {@render trailing?.()}
      {#if readonly !== null}{:else if stopMode}
        <button
          type="button"
          onclick={stop}
          disabled={stopping}
          aria-label="停止"
          title={stopping ? "正在停止" : "停止（Esc）"}
          class="grid size-9 shrink-0 place-items-center rounded-full bg-foreground text-background transition-opacity disabled:opacity-50">
          <Icon name={stopping ? "loader" : "stop"} class={stopping ? "size-5 animate-spin" : "size-4 fill-current"} />
        </button>
      {:else}
        <button
          type="button"
          onclick={send}
          disabled={!canSend}
          aria-label="发送"
          class="grid size-9 shrink-0 place-items-center rounded-full bg-primary text-primary-foreground transition-opacity disabled:opacity-30">
          <Icon name={posting ? "loader" : "arrowUp"} class={posting ? "size-5 animate-spin" : "size-5"} />
        </button>
      {/if}
    </div>
  </div>
</div>
