<script lang="ts">
  /** 三点跳动，表示模型正在工作；`size` 为单点直径（px），跳动幅度随之缩放。 */
  let { size = 6 }: { size?: number } = $props();
</script>

<span class="dots inline-flex shrink-0 items-center" style:--d="{size}px" aria-hidden="true"><i></i><i></i><i></i></span>

<style>
  .dots {
    gap: calc(var(--d) * 0.6);
    height: calc(var(--d) * 2);
  }
  .dots i {
    width: var(--d);
    height: var(--d);
    border-radius: 9999px;
    background: var(--primary);
    animation: hop 1.2s ease-in-out infinite;
  }
  .dots i:nth-child(2) { animation-delay: .15s; }
  .dots i:nth-child(3) { animation-delay: .3s; }
  @keyframes hop {
    0%, 60%, 100% { transform: translateY(0); opacity: .35; }
    30% { transform: translateY(calc(var(--d) * -0.6)); opacity: 1; }
  }
  @media (prefers-reduced-motion: reduce) {
    .dots i { animation: none; opacity: .6; }
  }
</style>
