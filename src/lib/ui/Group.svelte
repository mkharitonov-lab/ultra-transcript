<script lang="ts">
  import type { Snippet } from "svelte";

  /** Блок настроек: заголовок, пояснение и строки в общей рамке. `bare` — без рамки: содержимое рисует её само. */
  let { title = "", hint = "", bare = false, children }: { title?: string; hint?: string; bare?: boolean; children: Snippet } = $props();
</script>

<section class="group">
  {#if title}<h2>{title}</h2>{/if}
  {#if hint}<p class="muted hint">{hint}</p>{/if}
  <div class:rows={!bare}>{@render children()}</div>
</section>

<style>
  .group { margin-bottom: 26px; }
  h2 { margin: 0 2px 8px; }
  h2:has(+ .hint) { margin-bottom: 3px; }
  .hint { margin: 0 2px 9px; max-width: 620px; }
  .rows { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--bg-card); overflow: hidden; }
  .rows > :global(* + *) { border-top: 1px solid var(--border); }
</style>
