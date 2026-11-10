<script lang="ts">
  import Icon from "../lib/Icon.svelte";
  import Switch from "./Switch.svelte";

  let { checked, onchange }: { checked: boolean; onchange: () => void } = $props();
</script>

<label
  class="inline-flex h-9 shrink-0 items-center gap-1.5 rounded-full px-2 {checked ? 'text-destructive' : 'text-success'}"
  title={checked
    ? "凭据脱敏已临时关闭：开启后的新消息里的凭据原样发给模型，之前的内容仍脱敏；关掉恢复全部脱敏。主密钥与模型 key 始终遮住"
    : "凭据脱敏中：发给模型前遮住消息里的 key、token、密码等；打开开关可临时让新消息不脱敏"}>
  <Icon name={checked ? "lock-open" : "lock"} class="size-4" />
  <Switch {checked} label="临时关闭凭据脱敏" class={checked ? "bg-destructive" : "bg-success"} onclick={onchange} />
</label>
