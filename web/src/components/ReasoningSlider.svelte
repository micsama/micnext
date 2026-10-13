<script lang="ts" module>
  import type { ReasoningEffort } from "../api/types";

  const LABELS: Record<ReasoningEffort, string> = {
    none: "关闭思考", minimal: "极低", low: "低", medium: "中",
    high: "高", xhigh: "更高", max: "最高",
  };
  export const reasoningLabel = (effort: ReasoningEffort): string => LABELS[effort];
</script>

<script lang="ts">
  import type { ReasoningOptions } from "../api/types";

  let { options, value, onpick, disabled = false }: {
    options: ReasoningOptions | null;
    value: ReasoningEffort | null;
    onpick: (effort: ReasoningEffort | null) => void;
    disabled?: boolean;
  } = $props();

  const index = $derived(options ? options.choices.indexOf(value ?? options.default) : -1);
  let dragging = $state<number | null>(null);
  const active = $derived(dragging ?? index);
  const shown = $derived(options?.choices[active]);
  const progress = $derived(options && options.choices.length > 1 ? active / (options.choices.length - 1) * 100 : 0);

  function commit(event: Event) {
    const selected = Number((event.currentTarget as HTMLInputElement).value);
    dragging = null;
    onpick(options!.choices[selected]!);
  }
</script>

{#if options || value !== null}
  <div class="space-y-1">
    <div class="flex items-center justify-between gap-2">
      <div class="flex items-center gap-2">
        <span class="text-[11px] text-muted-foreground">思考强度</span>
        <span class="text-sm font-medium {shown ? 'text-foreground' : 'text-warning'}">
          {shown ? reasoningLabel(shown) : "档位已失效"}
        </span>
      </div>
      <button
        type="button"
        {disabled}
        aria-label="使用模型默认思考强度"
        aria-pressed={value === null}
        class="rounded-full px-2 py-0.5 text-[10px] transition-colors hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring {value === null ? 'bg-muted text-muted-foreground' : 'text-muted-foreground/70'}"
        onclick={() => { dragging = null; onpick(null); }}>默认</button>
    </div>
    {#if options && index >= 0 && options.choices.length > 1}
      <div class="relative h-9" style={`--progress: ${progress}%`}>
        <input
          type="range"
          min="0"
          max={options.choices.length - 1}
          step="1"
          value={active}
          {disabled}
          aria-label="思考强度"
          aria-valuetext={shown ? `${reasoningLabel(shown)}${dragging === null && value === null ? '（默认）' : ''}` : "档位已失效"}
          class="absolute inset-0 h-9 w-full cursor-pointer"
          oninput={(e) => (dragging = Number(e.currentTarget.value))}
          onchange={commit}
          onpointercancel={() => (dragging = null)} />
        <div class="pointer-events-none absolute inset-x-3 top-1/2 flex -translate-y-1/2 justify-between">
          {#each options.choices as effort, i (effort)}
            <span title={reasoningLabel(effort)} class="size-1 rounded-full {i <= active ? 'bg-white/60' : 'bg-muted-foreground/30'}"></span>
          {/each}
        </div>
      </div>
      <div class="flex justify-between px-0.5 text-[10px] text-muted-foreground/70">
        <span>{reasoningLabel(options.choices[0]!)}</span>
        <span>{reasoningLabel(options.choices.at(-1)!)}</span>
      </div>
    {/if}
  </div>
{/if}

<style>
  input { appearance: none; background: transparent; }
  input:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; border-radius: 999px; }
  input::-webkit-slider-runnable-track {
    height: 16px;
    border-radius: 999px;
    background: linear-gradient(to right, color-mix(in srgb, var(--primary) 65%, white) 0%, var(--primary) var(--progress), var(--accent) var(--progress), var(--accent) 100%);
  }
  input::-webkit-slider-thumb {
    appearance: none;
    width: 24px;
    height: 24px;
    margin-top: -4px;
    border: 1px solid rgb(0 0 0 / 9%);
    border-radius: 50%;
    background: white;
    box-shadow: 0 2px 5px rgb(0 0 0 / 12%);
  }
  input::-moz-range-track {
    height: 16px;
    border-radius: 999px;
    background: linear-gradient(to right, color-mix(in srgb, var(--primary) 65%, white) 0%, var(--primary) var(--progress), var(--accent) var(--progress), var(--accent) 100%);
  }
  input::-moz-range-thumb {
    width: 24px;
    height: 24px;
    border: 1px solid rgb(0 0 0 / 9%);
    border-radius: 50%;
    background: white;
    box-shadow: 0 2px 5px rgb(0 0 0 / 12%);
  }
</style>
