<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import CircleAlertIcon from "@lucide/svelte/icons/circle-alert";
  import LoaderIcon from "@lucide/svelte/icons/loader-circle";
  import MessageCircleIcon from "@lucide/svelte/icons/message-circle";
  import MessagesSquareIcon from "@lucide/svelte/icons/messages-square";
  import SmartphoneIcon from "@lucide/svelte/icons/smartphone";
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import QRCode from "$lib/vendor/qrcode.js";
  import { beginWechatLogin, cancelWechatLogin, getWechat, submitWechatCode } from "../../api/client";
  import type { SetupProgress, WechatView } from "../../api/types";
  import { router } from "../../state/route.svelte";
  import Group from "./Group.svelte";

  let view = $state<WechatView | null>(null);
  let error = $state("");
  let busy = $state(false);
  let code = $state("");
  let qrImage = $state("");
  let qrError = $state("");
  let requestId = 0;

  const account = $derived(view?.available ? view.account : null);
  const attempt = $derived(view?.available ? view.login : null);
  const progress = $derived(attempt?.progress);
  const qrContent = $derived(progress && "qr_content" in progress ? progress.qr_content : "");
  const active = $derived(progress && ["preparing", "waiting", "scanned", "needs_code"].includes(progress.state));

  async function refresh() {
    const id = ++requestId;
    try {
      const next = await getWechat();
      if (id !== requestId) return;
      view = next;
      error = "";
    } catch (e) {
      if (id === requestId) error = (e as Error).message;
    }
  }

  onMount(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      await refresh();
      if (!disposed) timer = setTimeout(poll, 1500);
    };
    void poll();
    return () => { disposed = true; ++requestId; clearTimeout(timer); };
  });

  $effect(() => {
    const content = qrContent;
    let disposed = false;
    qrImage = "";
    qrError = "";
    if (content) {
      QRCode.toDataURL(content, { width: 240, margin: 4, errorCorrectionLevel: "M" })
        .then((image) => { if (!disposed) qrImage = image; })
        .catch(() => { if (!disposed) qrError = "二维码生成失败，请重新获取"; });
    }
    return () => { disposed = true; };
  });

  async function act(operation: () => Promise<unknown>) {
    busy = true;
    error = "";
    try { await operation(); await refresh(); }
    catch (e) { error = (e as Error).message; }
    finally { busy = false; }
  }

  const connections = {
    connected: { text: "已连接", dot: "bg-success" },
    needs_login: { text: "需要重新登录", dot: "bg-warning" },
    faulted: { text: "连接异常", dot: "bg-destructive" },
  } as const;

  function description(p: SetupProgress): string {
    switch (p.state) {
      case "preparing": return "正在获取二维码…";
      case "waiting": return "用手机微信扫码";
      case "scanned": return "已扫码，等待确认…";
      case "needs_code": return "输入手机微信显示的验证码";
      case "expired": return "二维码已过期";
      case "cancelled": return "已取消登录";
      case "connected": return "连接成功";
      case "failed": return {
        network: "连接失败，请重试",
        verification_blocked: "验证码暂不可用，请重新获取二维码",
        existing_binding: "已有绑定，请重新获取二维码登录",
        protocol: "微信接口不匹配，需要修复",
      }[p.reason];
    }
  }
</script>

{#snippet tile(Icon: typeof SmartphoneIcon, tone: string)}
  <div class="flex size-9 shrink-0 items-center justify-center rounded-lg {tone}"><Icon class="size-4" /></div>
{/snippet}

<div class="space-y-8">
  {#if view && !view.available}
    <Group>
      <p class="px-4 py-6 text-center text-sm text-muted-foreground">本构建未包含微信</p>
    </Group>
  {:else if view?.available}
    {@const conn = account ? connections[account.connection] : { text: "未连接", dot: "bg-muted-foreground/40" }}
    <Group title="账号">
      <div class="flex items-center gap-3 px-4 py-3.5">
        {@render tile(MessageCircleIcon, "bg-[#07c160]/12 text-[#07c160]")}
        <div class="min-w-0 flex-1">
          <p class="flex items-center gap-1.5 text-sm font-medium">
            <span class="size-1.5 shrink-0 rounded-full {conn.dot}"></span>{conn.text}
          </p>
          <p class="mt-0.5 text-xs text-muted-foreground">
            {account ? "只回复扫码绑定的这个微信" : "扫码登录后，即可在微信里与 micnext 对话"}
          </p>
        </div>
        {#if !active}
          <Button variant={account?.connection === "connected" ? "ghost" : "default"} size="sm" class="shrink-0" disabled={busy} onclick={() => act(beginWechatLogin)}>
            {progress && progress.state !== "connected" ? "重新获取二维码" : account ? "重新登录" : "扫码连接"}
          </Button>
        {:else if attempt}
          <Button variant="ghost" size="sm" class="shrink-0" disabled={busy} onclick={() => act(() => cancelWechatLogin(attempt!.id))}>取消</Button>
        {/if}
      </div>
      {#if account}
        <button
          type="button"
          class="flex w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-accent/60"
          onclick={() => router.go(`/s/${account!.session_id}`)}
        >
          {@render tile(MessagesSquareIcon, "bg-muted text-muted-foreground")}
          <div class="min-w-0 flex-1">
            <p class="text-sm">微信会话</p>
            <p class="mt-0.5 text-xs text-muted-foreground">查看微信里的对话，可在会话中切换模型</p>
          </div>
          <ChevronRightIcon class="size-4 shrink-0 text-muted-foreground" />
        </button>
      {/if}
    </Group>

    {#if progress}
      {@const tone = progress.state === "connected" ? "text-success" : progress.state === "failed" || progress.state === "expired" ? "text-destructive" : "text-muted-foreground"}
      <Group title="扫码登录">
        <div class="flex flex-col items-center gap-4 px-4 py-6">
          {#if qrContent}
            <div class="flex size-[216px] items-center justify-center rounded-2xl border bg-white p-2 shadow-sm">
              {#if qrImage}
                <img src={qrImage} alt="微信登录二维码" width="200" height="200" class="size-[200px] {progress.state === 'scanned' ? 'opacity-30' : ''}" />
              {:else}
                <LoaderIcon class="size-5 animate-spin text-neutral-400" />
              {/if}
            </div>
          {/if}
          <p class="flex items-center gap-1.5 text-sm {tone}">
            {#if progress.state === "preparing" || progress.state === "scanned"}
              <LoaderIcon class="size-4 animate-spin" />
            {:else if progress.state === "connected"}
              <CheckIcon class="size-4" />
            {:else if progress.state === "failed" || progress.state === "expired"}
              <CircleAlertIcon class="size-4" />
            {:else if progress.state === "waiting"}
              <SmartphoneIcon class="size-4" />
            {/if}
            {description(progress)}
          </p>
          {#if qrError}<p class="text-sm text-destructive">{qrError}</p>{/if}
          {#if progress.state === "needs_code" && attempt}
            <form class="flex w-full max-w-xs gap-2" onsubmit={(event) => {
              event.preventDefault();
              void act(async () => { await submitWechatCode(attempt!.id, code); code = ""; });
            }}>
              <Input aria-label="微信验证码" autocomplete="one-time-code" class="text-center tracking-widest tabular-nums" placeholder="验证码" bind:value={code} />
              <Button type="submit" disabled={busy || !code.trim()}>确认</Button>
            </form>
          {/if}
        </div>
      </Group>
    {/if}
  {:else}
    <p class="text-sm text-muted-foreground">加载中…</p>
  {/if}
  {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
</div>
