<script lang="ts">
  /** Полоса выполнения; без `value` (или с нулём) — бегущая: идёт работа, доля неизвестна. */
  let { value = 0, width = "100%" }: { value?: number; width?: string } = $props();
  const known = $derived(value > 0);
</script>

<div class="bar" class:busy={!known} style="width:{width}" role="progressbar" aria-valuemin={0} aria-valuemax={100}
  aria-valuenow={known ? Math.round(value * 100) : undefined}>
  <span style={known ? `width:${Math.min(100, Math.round(value * 100))}%` : ""}></span>
</div>

<style>
  .bar { height: 5px; border-radius: 3px; background: var(--bg-active); overflow: hidden; }
  span { display: block; height: 100%; border-radius: 3px; background: var(--accent); transition: width 0.3s; }
  .busy span { width: 35%; animation: run 1.3s ease-in-out infinite; }
  @keyframes run { from { transform: translateX(-100%); } to { transform: translateX(290%); } }
  @media (prefers-reduced-motion: reduce) { .busy span { animation: none; width: 100%; opacity: 0.4; } }
</style>
