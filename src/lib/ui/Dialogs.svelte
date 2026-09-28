<script lang="ts">
  import { tick } from "svelte";
  import { t } from "../i18n.svelte";
  import { dialogs } from "./dialog.svelte";

  let value = $state("");
  let box = $state<HTMLElement>();
  const d = $derived(dialogs.current);
  const asks = $derived(d?.input !== undefined);

  $effect(() => {
    if (!d) return;
    value = d.input ?? "";
    tick().then(() => {
      const field = box?.querySelector<HTMLInputElement>("input");
      if (field) {
        field.focus();
        field.select();
      } else box?.querySelector<HTMLElement>("button.main")?.focus();
    });
  });

  const cancel = () => dialogs.answer(asks ? null : false);
  function accept() {
    if (asks && !value.trim()) return;
    dialogs.answer(asks ? value.trim() : true);
  }

  function onkeydown(e: KeyboardEvent) {
    if (!d) return;
    if (e.key === "Escape") cancel();
    else if (e.key === "Enter" && !e.isComposing) accept();
    else if (e.key === "Tab") {
      // Фокус не выходит за пределы диалога.
      const els = [...(box?.querySelectorAll<HTMLElement>("input, button") ?? [])];
      const i = els.indexOf(document.activeElement as HTMLElement);
      els[(i + (e.shiftKey ? -1 : 1) + els.length) % els.length]?.focus();
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<svelte:window onkeydowncapture={onkeydown} />

{#if d}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="scrim" onmousedown={(e) => { if (e.target === e.currentTarget) cancel(); }}>
    <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="dialog-title" bind:this={box}>
      <h2 id="dialog-title">{d.title}</h2>
      {#if d.text}<p class="muted">{d.text}</p>{/if}
      {#if asks}<input type="text" bind:value placeholder={d.placeholder ?? ""} spellcheck="false" />{/if}
      <div class="buttons">
        {#if d.cancel !== null}<button onclick={cancel}>{d.cancel ?? t("common.cancel")}</button>{/if}
        <button class="main {d.danger ? 'destructive' : 'primary'}" onclick={accept} disabled={asks && !value.trim()}>
          {d.confirm ?? t("common.ok")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 700; display: grid; place-items: center; padding: 24px;
    background: var(--scrim); animation: fade 0.14s ease-out;
  }
  .dialog {
    width: min(400px, 100%); padding: 20px; border-radius: var(--r-xl); background: var(--bg-elevated);
    border: 1px solid var(--border); box-shadow: var(--shadow-lg); animation: rise 0.18s var(--ease);
  }
  h2 { font-size: var(--fs-lg); margin: 0; }
  p { margin: 6px 0 0; white-space: pre-wrap; overflow-wrap: anywhere; user-select: text; -webkit-user-select: text; }
  input { margin-top: 14px; padding: 7px 10px; }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; margin-top: 20px; }
  .buttons button { min-width: 84px; padding: 6px 14px; }
  @keyframes fade { from { opacity: 0; } }
  @keyframes rise { from { opacity: 0; transform: translateY(8px) scale(0.98); } }
  @media (prefers-reduced-motion: reduce) { .scrim, .dialog { animation: none; } }
</style>
