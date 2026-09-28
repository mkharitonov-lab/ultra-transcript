<script lang="ts">
  import { onMount } from "svelte";
  import { api, type AppInfo } from "./api";

  let { info, onmodels }: { info: AppInfo; onmodels: () => void } = $props();
  let progress = $state<Record<string, number>>({});
  let error = $state("");
  let busy = $state(false);

  const missing = $derived(info.models.filter((m) => !m.installed));
  const totalMb = $derived(missing.reduce((a, m) => a + m.size_mb, 0));

  onMount(() => {
    const un = api.onModels((e) => {
      if (e.error) { error = e.error; busy = false; return; }
      progress[e.name] = e.progress;
      if (e.progress >= 1) onmodels();
    });
    return () => { un.then((f) => f()); };
  });

  $effect(() => { if (!missing.length) busy = false; });

  function install() {
    error = ""; busy = true;
    api.installModels();
  }
</script>

<div class="card">
  <h2>Модели</h2>
  {#each info.models as m}
    <div class="row">
      <span class="dot" class:ok={m.installed}></span>
      <span class="title">{m.title}</span>
      {#if m.installed}<span class="faint">установлена</span>
      {:else if progress[m.name] !== undefined}
        <span class="bar"><span style="width:{Math.round(progress[m.name] * 100)}%"></span></span>
      {:else}<span class="faint">{m.size_mb} МБ</span>{/if}
    </div>
  {/each}
  {#if missing.length}
    <div class="actions">
      <button class="primary" onclick={install} disabled={busy}>
        {busy ? "Скачивание…" : `Скачать модели (≈${totalMb} МБ)`}
      </button>
      <span class="faint">Всё работает локально, аудио никуда не отправляется.</span>
    </div>
  {/if}
  {#if error}<p class="err">{error}</p>{/if}
</div>

<style>
  .card { background: var(--bg-card); border: 1px solid var(--border); border-radius: 12px; padding: 16px 18px; }
  .row { display: flex; align-items: center; gap: 10px; padding: 5px 0; }
  .title { flex: 1; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--fg-faint); }
  .dot.ok { background: var(--ok); }
  .bar { width: 120px; height: 6px; border-radius: 3px; background: var(--border); overflow: hidden; }
  .bar span { display: block; height: 100%; background: var(--accent); transition: width 0.2s; }
  .actions { display: flex; align-items: center; gap: 12px; margin-top: 12px; }
  .err { color: var(--danger); margin: 10px 0 0; }
</style>
