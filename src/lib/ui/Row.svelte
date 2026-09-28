<script lang="ts">
  import type { Snippet } from "svelte";

  /** Строка настроек: подпись с пояснением слева, элемент управления справа (или ниже — `stack`). */
  let { label, hint = "", stack = false, children }: { label: string; hint?: string; stack?: boolean; children?: Snippet } = $props();
</script>

<div class="row" class:stack>
  <div class="text">
    <div class="label">{label}</div>
    {#if hint}<div class="hint muted">{hint}</div>{/if}
  </div>
  {#if children}<div class="control">{@render children()}</div>{/if}
</div>

<style>
  .row { display: flex; align-items: center; gap: 16px; padding: 10px 14px; min-height: 44px; }
  .row.stack { flex-direction: column; align-items: stretch; gap: 8px; }
  .text { flex: 1; min-width: 0; }
  .label { font-weight: 500; }
  .hint { font-size: var(--fs-sm); margin-top: 1px; max-width: 560px; }
  .control { display: flex; align-items: center; gap: 8px; flex-shrink: 0; max-width: 60%; }
  .stack .control { max-width: none; }
</style>
