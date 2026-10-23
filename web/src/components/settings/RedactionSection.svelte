<script lang="ts">
  import { getRedactRules, putRedactRules } from "../../api/client";
  import Switch from "../Switch.svelte";
  import Group from "./Group.svelte";
  import SaveStatus, { type SaveState } from "./SaveStatus.svelte";

  let rules = $state<boolean | null>(null);
  let status = $state<SaveState | undefined>();

  $effect(() => {
    getRedactRules().then(
      (r) => (rules = r),
      (e: Error) => (status = { kind: "error", message: e.message }),
    );
  });

  async function toggle() {
    if (rules === null || status?.kind === "saving") return;
    status = { kind: "saving" };
    try {
      await putRedactRules(!rules);
      rules = !rules;
      status = { kind: "saved" };
    } catch (e) {
      status = { kind: "error", message: (e as Error).message };
    }
  }
</script>

<Group
  title="隐私"
  footer="发给模型前遮住常见 key、token、密码与私钥。关闭后主密钥与模型自己的 API key 仍会遮住。下一次模型调用生效。"
>
  {#snippet actions()}<SaveStatus state={status} />{/snippet}
  <div class="flex items-center justify-between gap-3 px-4 py-3.5">
    <span class="text-sm">按规则脱敏</span>
    {#if rules !== null}
      <Switch checked={rules} label="按规则脱敏" disabled={status?.kind === "saving"} onclick={toggle} />
    {/if}
  </div>
  {#if status?.kind === "error"}<p role="alert" class="px-4 pb-3 text-xs text-destructive">{status.message}</p>{/if}
</Group>
