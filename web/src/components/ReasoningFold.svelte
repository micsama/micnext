<script lang="ts">
  let { text }: { text: string } = $props();
</script>

<details class="group/rs my-1 text-sm">
  <summary class="cursor-pointer list-none text-muted select-none hover:text-fg">
    思考过程 <span class="inline-block transition-transform group-open/rs:rotate-90">▸</span>
  </summary>
  <div class="mt-1 border-l-2 border-line pl-3 whitespace-pre-wrap text-muted">{text}</div>
</details>
