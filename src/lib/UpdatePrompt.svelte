<script lang="ts">
  import { t } from "./i18n.svelte";
  import { updater } from "./updater.svelte";

  const shown = $derived(
    !!updater.update && !updater.dismissed && ["available", "downloading", "error"].includes(updater.status),
  );
</script>

{#if shown && updater.update}
  <div class="update" role="dialog" aria-label={t("update.title", { version: updater.update.version })}>
    <h2>{t("update.title", { version: updater.update.version })}</h2>
    {#if updater.notes.length}
      <ul>
        {#each updater.notes as line, i (i)}<li>{line}</li>{/each}
      </ul>
    {/if}
    {#if updater.status === "downloading"}
      <div class="bar" role="progressbar" aria-valuenow={Math.round(updater.progress * 100)}>
        <div style:width="{Math.max(4, updater.progress * 100)}%"></div>
      </div>
      <p class="muted">{t("update.installing")}</p>
    {:else}
      {#if updater.status === "error"}<p class="error">{t("update.failed")}</p>{/if}
      <div class="buttons">
        <button onclick={() => (updater.dismissed = true)}>{t("update.later")}</button>
        <button class="primary" onclick={() => updater.install()}>{t("update.install")}</button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .update {
    position: fixed; right: 16px; bottom: 16px; z-index: 750; width: min(380px, calc(100vw - 32px));
    padding: 16px 16px 14px; border-radius: var(--r-xl); background: var(--bg-elevated);
    border: 1px solid var(--border); box-shadow: var(--shadow-lg); animation: rise 0.18s var(--ease);
  }
  h2 { font-size: var(--fs-lg); margin: 0; }
  ul { margin: 8px 0 0; padding-left: 18px; max-height: 40vh; overflow-y: auto; user-select: text; -webkit-user-select: text; }
  li + li { margin-top: 3px; }
  p { margin: 8px 0 0; }
  .error { color: var(--danger); }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
  .buttons button { padding: 6px 14px; }
  .bar { margin-top: 14px; height: 4px; border-radius: 2px; background: var(--border); overflow: hidden; }
  .bar div { height: 100%; background: var(--accent); transition: width 0.2s; }
  @keyframes rise { from { opacity: 0; transform: translateY(8px); } }
  @media (prefers-reduced-motion: reduce) { .update { animation: none; } }
</style>
