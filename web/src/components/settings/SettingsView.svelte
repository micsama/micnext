<script lang="ts">
  import ArchiveIcon from "@lucide/svelte/icons/archive";
  import BackupSection from "./BackupSection.svelte";
  import BoxesIcon from "@lucide/svelte/icons/boxes";
  import KeyRoundIcon from "@lucide/svelte/icons/key-round";
  import SlidersIcon from "@lucide/svelte/icons/sliders-horizontal";
  import SparklesIcon from "@lucide/svelte/icons/sparkles";
  import TagIcon from "@lucide/svelte/icons/tag";
  import QrCodeIcon from "@lucide/svelte/icons/qr-code";
  import type { Component } from "svelte";
  import { MediaQuery } from "svelte/reactivity";
  import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import { cn } from "$lib/utils";
  import { settingsStore } from "../../state/settings.svelte";
  import { router, type SettingsTab } from "../../state/route.svelte";
  import Header from "../Header.svelte";
  import EndpointsSection from "./EndpointsSection.svelte";
  import EnvVarsSection from "./EnvVarsSection.svelte";
  import PersonasSection from "./PersonasSection.svelte";
  import PrefsSection from "./PrefsSection.svelte";
  import TagsSection from "./TagsSection.svelte";
  import WechatSection from "./WechatSection.svelte";

  $effect(() => {
    document.title = "设置 · micnext";
  });

  const tabs: { id: SettingsTab; label: string; hint: string; icon: Component; group: string }[] = [
    { id: "prefs", label: "对话偏好", hint: "默认人设与模型、杂活模型、推送渠道", icon: SlidersIcon, group: "对话" },
    { id: "personas", label: "人设", hint: "人设内容与图标", icon: SparklesIcon, group: "对话" },
    { id: "tags", label: "标签", hint: "会话分类与筛选", icon: TagIcon, group: "对话" },
    { id: "models", label: "服务商与模型", hint: "连接、模型与计费单价", icon: BoxesIcon, group: "模型与运行" },
    { id: "env", label: "环境变量", hint: "命令里可直接引用的变量", icon: KeyRoundIcon, group: "模型与运行" },
    { id: "wechat", label: "微信", hint: "扫码登录", icon: QrCodeIcon, group: "连接与数据" },
    { id: "backup", label: "设置备份", hint: "导出、导入全部设置", icon: ArchiveIcon, group: "连接与数据" },
  ];
  const groups = [...new Set(tabs.map((t) => t.group))];
  /** 手机：未选分类时显示目录。 */
  const narrow = new MediaQuery("max-width: 767.98px");
  const route = $derived(router.current.kind === "settings" ? router.current : null);
  const priceModel = $derived(route?.model ?? null);
  const tab = $derived(route?.tab ?? (narrow.current ? null : "prefs"));
  const current = $derived(tabs.find((t) => t.id === tab) ?? null);

  // 弹窗遮挡站内导航，只需拦截刷新与关闭页面。
  let personasDirty = $state(false);
  let endpointsDirty = $state(false);
  $effect(() => {
    if (!personasDirty && !endpointsDirty) return;
    const onbeforeunload = (e: BeforeUnloadEvent) => e.preventDefault();
    addEventListener("beforeunload", onbeforeunload);
    return () => removeEventListener("beforeunload", onbeforeunload);
  });
</script>

<Header title={narrow.current && current ? current.label : "设置"} />
{#if current === null}
  <div class="min-h-0 flex-1 overflow-y-auto px-4 pt-2 pb-[max(1.5rem,env(safe-area-inset-bottom))]">
    {#each groups as g (g)}
      <p class="mt-4 mb-1.5 px-3 text-xs text-muted-foreground">{g}</p>
      <div class="overflow-hidden rounded-xl border bg-card">
        {#each tabs.filter((t) => t.group === g) as t (t.id)}
          <button type="button" class="flex min-h-14 w-full items-center gap-3 border-b px-3.5 text-left last:border-b-0 active:bg-accent" onclick={() => router.go(`/settings/${t.id}`)}>
            <t.icon class="size-5 shrink-0 text-muted-foreground" />
            <span class="min-w-0 flex-1">
              <span class="block text-[15px] font-medium">{t.label}</span>
              <span class="block truncate text-xs text-muted-foreground">{t.hint}</span>
            </span>
            <ChevronRightIcon class="size-4 shrink-0 text-muted-foreground" />
          </button>
        {/each}
      </div>
    {/each}
  </div>
{:else}
<div class="flex min-h-0 flex-1 flex-col md:flex-row">
  <nav class="flex shrink-0 flex-col gap-1 border-r p-3 max-md:hidden md:w-52">
    {#each tabs as t (t.id)}
      <button
        type="button"
        class={cn(
          "flex shrink-0 items-center gap-2.5 rounded-lg px-3 py-1.5 text-sm transition-colors",
          t.id === tab ? "bg-accent font-medium text-foreground" : "text-muted-foreground hover:text-foreground",
        )}
        aria-current={t.id === tab ? "page" : undefined}
        onclick={() => router.go(`/settings/${t.id}`)}
      >
        <t.icon class="size-4" />
        {t.label}
      </button>
    {/each}
  </nav>

  <div class="min-h-0 flex-1 overflow-y-auto">
    <div class="mx-auto max-w-2xl px-4 pt-2 pb-[max(1.5rem,env(safe-area-inset-bottom))] md:px-8 md:py-10">
      <button type="button" class="-ml-2 mb-2 flex h-11 items-center gap-0.5 px-2 text-[15px] text-primary md:hidden" onclick={() => router.go("/settings")}>
        <ChevronLeftIcon class="size-5" />全部设置
      </button>
      <h2 class="mb-6 text-2xl font-semibold tracking-tight max-md:hidden">{current.label}</h2>
      {#if settingsStore.error}
        <p class="mb-4 text-sm text-destructive">{settingsStore.error}</p>
      {/if}
      {#if tab === "backup"}
        <BackupSection />
      {:else if tab === "tags"}
        <TagsSection />
      {:else if tab === "env"}
        <EnvVarsSection />
      {:else if tab === "wechat"}
        <WechatSection />
      {:else if !settingsStore.settings}
        <p class="text-sm text-muted-foreground">加载中…</p>
      {:else if tab === "prefs"}
        <PrefsSection settings={settingsStore.settings} />
      {:else if tab === "personas"}
        <PersonasSection ondirty={(d) => (personasDirty = d)} />
      {:else}
        <EndpointsSection focus={priceModel} ondirty={(d) => (endpointsDirty = d)} />
      {/if}
    </div>
  </div>
</div>
{/if}
