<script lang="ts">
  let { text }: { text: string } = $props();
</script>

<details class="group/rs my-1 text-sm">
  <summary class="cursor-pointer list-none text-muted-foreground select-none hover:text-foreground">
    思考过程 <span class="inline-block transition-transform group-open/rs:rotate-90">▸</span>
  </summary>
  <div class="mt-1 border-l-2 border-border pl-3 whitespace-pre-wrap text-muted-foreground">{text}</div>
</details>
