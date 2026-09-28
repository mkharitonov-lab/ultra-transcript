<script lang="ts">
  import type { Snippet } from "svelte";
  let { title, subtitle = "", actions, children }: { title: string; subtitle?: string; actions?: Snippet; children: Snippet } = $props();
</script>

<div class="page">
  <header>
    <div>
      <h1>{title}</h1>
      {#if subtitle}<p class="muted">{subtitle}</p>{/if}
    </div>
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
  </header>
  <div class="body">{@render children()}</div>
</div>

<style>
  .page { display: flex; flex-direction: column; height: 100vh; min-height: 0; }
  header { display: flex; align-items: flex-end; justify-content: space-between; gap: 16px; padding: 38px 28px 14px; border-bottom: 1px solid var(--border); }
  header p { margin: 4px 0 0; max-width: 640px; }
  .actions { display: flex; gap: 8px; align-items: center; }
  .body { flex: 1; overflow-y: auto; padding: 16px 28px 40px; }
  :global(.grid) { width: 100%; border-collapse: collapse; }
  :global(.grid th) { text-align: left; font-size: 11px; font-weight: 600; color: var(--fg-faint); text-transform: uppercase; letter-spacing: .04em; padding: 0 6px 6px; }
  :global(.grid td) { padding: 3px; vertical-align: top; }
  :global(.grid td input) { border-color: transparent; background: transparent; }
  :global(.grid tr:hover td input) { border-color: var(--border); background: var(--bg-input); }
  :global(.grid td.tools) { width: 1%; white-space: nowrap; }
  :global(.empty-state) { color: var(--fg-muted); padding: 30px 0; text-align: center; }
</style>
