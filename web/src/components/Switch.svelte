<script lang="ts">
  import { cn } from "$lib/utils";

  let {
    checked,
    label,
    title,
    disabled = false,
    class: className,
    onclick,
  }: { checked: boolean; label: string; title?: string; disabled?: boolean; class?: string; onclick: () => void } =
    $props();
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {title}
  {disabled}
  {onclick}
  class={cn(
    "relative h-5 w-9 shrink-0 rounded-full transition-colors focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-50",
    checked ? "bg-primary" : "bg-muted-foreground/30",
    className,
  )}
  ><span
    class={cn("absolute top-0.5 left-0.5 size-4 rounded-full bg-background shadow transition-transform", checked && "translate-x-4")}
  ></span></button
>
