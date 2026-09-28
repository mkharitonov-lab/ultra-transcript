<script lang="ts">
  import type { Snippet } from "svelte";
  let { title, subtitle = "", actions, children }: { title: string; subtitle?: string; actions?: Snippet; children: Snippet } = $props();
</script>

<div class="page">
  <header data-tauri-drag-region>
    <div class="heading">
      <h1>{title}</h1>
      {#if subtitle}<p class="muted">{subtitle}</p>{/if}
    </div>
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
  </header>
  <div class="body">{@render children()}</div>
</div>

<style>
  .page { display: flex; flex-direction: column; height: 100vh; min-height: 0; }
  header {
    display: flex; flex-wrap: wrap; align-items: flex-end; justify-content: space-between; gap: 12px 16px;
    padding: 38px 28px 14px; border-bottom: 1px solid var(--border); position: relative; z-index: 6;
  }
  /* В узком окне кнопки уходят на следующую строку, а не сжимают заголовок в колонку. */
  .heading { flex: 1 1 320px; pointer-events: none; }
  header p { margin: 4px 0 0; max-width: 640px; }
  .actions { display: flex; gap: 8px; align-items: center; margin-left: auto; }
  .body { flex: 1; overflow-y: auto; padding: 16px 28px 40px; }
</style>
