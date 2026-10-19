<script lang="ts">
  import Icon from "../lib/Icon.svelte";

  let { text }: { text: string } = $props();
</script>

<details class="group/rs my-1 text-sm">
  <summary class="flex w-fit cursor-pointer list-none items-center gap-1.5 text-xs text-muted-foreground select-none hover:text-foreground [&::-webkit-details-marker]:hidden">
    <Icon name="bulb" class="size-3.5" />思考过程<Icon name="chevron" class="size-3 transition-transform group-open/rs:rotate-90" />
  </summary>
  <div class="mt-1 border-l-2 border-border pl-3 whitespace-pre-wrap text-muted-foreground">{text}</div>
</details>
