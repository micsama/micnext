<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import QRCode from "$lib/vendor/qrcode.js";
  import { beginWechatLogin, cancelWechatLogin, getWechat, submitWechatCode } from "../../api/client";
  import type { SetupProgress, WechatView } from "../../api/types";
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

<Group>
  <div class="space-y-5 p-5">
    {#if view && !view.available}
      <p class="text-sm text-muted-foreground">本构建未包含微信</p>
    {:else if view?.available}
      <div class="flex items-center justify-between gap-4">
        <div class="min-w-0 space-y-1">
          <p class="text-sm font-medium">{account ? ({ connected: "已连接", needs_login: "需要重新登录", faulted: "连接异常" }[account.connection]) : "未连接"}</p>
          {#if account}
            <p class="break-all text-xs text-muted-foreground">{account.user_id}</p>
          {/if}
        </div>
        {#if !active}
          <Button variant="outline" disabled={busy} onclick={() => act(beginWechatLogin)}>
            {progress && progress.state !== "connected" ? "重新获取二维码" : account ? "重新登录" : "扫码连接"}
          </Button>
        {:else if attempt}
          <Button variant="ghost" disabled={busy} onclick={() => act(() => cancelWechatLogin(attempt!.id))}>取消</Button>
        {/if}
      </div>

      {#if progress}
        <div class="space-y-3 border-t pt-5">
          {#if qrImage}
            <img src={qrImage} alt="微信登录二维码" width="240" height="240" class="mx-auto rounded-lg" />
          {/if}
          <p class="text-center text-sm text-muted-foreground">{description(progress)}</p>
          {#if qrError}<p class="text-center text-sm text-destructive">{qrError}</p>{/if}
          {#if progress.state === "needs_code" && attempt}
            <form class="mx-auto flex max-w-xs gap-2" onsubmit={(event) => {
              event.preventDefault();
              void act(async () => { await submitWechatCode(attempt!.id, code); code = ""; });
            }}>
              <Input aria-label="微信验证码" autocomplete="one-time-code" bind:value={code} />
              <Button type="submit" disabled={busy || !code.trim()}>确认</Button>
            </form>
          {/if}
        </div>
      {/if}
    {:else}
      <p class="text-sm text-muted-foreground">加载中…</p>
    {/if}
    {#if error}<p class="text-sm text-destructive">{error}</p>{/if}
  </div>
</Group>
