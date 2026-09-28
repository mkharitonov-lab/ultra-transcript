<script lang="ts">
  import { fly } from "svelte/transition";
  import Icon from "../Icon.svelte";
  import { t } from "../i18n.svelte";
  import { toasts } from "./toast.svelte";
</script>

<div class="toasts" aria-live="polite">
  {#each toasts.items as item (item.id)}
    <div class="toast {item.kind}" role={item.kind === "error" ? "alert" : "status"} transition:fly={{ y: 12, duration: 160 }}>
      <span class="ico"><Icon name={item.kind === "error" ? "alert" : item.kind === "ok" ? "check" : "info"} size={15} /></span>
      <span class="text">{item.text}</span>
      {#if item.action}
        <button class="ghost act" onclick={() => { item.action!.run(); toasts.dismiss(item.id); }}>{item.action.label}</button>
      {/if}
      <button class="ghost close" onclick={() => toasts.dismiss(item.id)} aria-label={t("common.close")}><Icon name="x" size={12} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed; right: 16px; bottom: 16px; z-index: 800; display: flex; flex-direction: column; gap: 8px;
    align-items: flex-end; pointer-events: none; max-width: min(460px, calc(100vw - 32px));
  }
  .toast {
    pointer-events: auto; display: flex; align-items: flex-start; gap: 9px; padding: 10px 10px 10px 12px;
    border-radius: var(--r-lg); background: var(--bg-elevated); border: 1px solid var(--border); box-shadow: var(--shadow-md);
  }
  .ico { display: grid; margin-top: 2px; color: var(--fg-muted); flex-shrink: 0; }
  .ok .ico { color: var(--ok); }
  .error .ico { color: var(--danger); }
  .text {
    flex: 1; user-select: text; -webkit-user-select: text; cursor: text; white-space: pre-wrap; overflow-wrap: anywhere;
    max-height: 9em; overflow-y: auto;
  }
  .act { color: var(--accent); font-weight: 550; padding: 1px 8px; }
  .close { padding: 4px; display: grid; color: var(--fg-faint); }
</style>
