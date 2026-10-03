<script lang="ts">
  import Icon from "./Icon.svelte";
  import { api, fmtTime, showError, type LiveState, type Recording } from "./api";
  import { t } from "./i18n.svelte";
  import { live } from "./live.svelte";
  import { app } from "./state.svelte";
  import { confirm } from "./ui/dialog.svelte";

  /** Запись с микрофона: таймер, громкость, кнопка остановки и текст по мере распознавания. */
  let { recording, job }: { recording: Recording; job?: LiveState } = $props();

  const peak = $derived(live.peak(recording.id));
  const seconds = $derived(job?.seconds ?? 0);
  /** Модели ещё грузятся: звук пока не пишется. */
  const starting = $derived(!job || job.stage !== "record");
  /** Самое громкое за всё время — чтобы отличить тишину от неработающего микрофона. */
  let loudest = $state(0);
  $effect(() => {
    if (peak > loudest) loudest = peak;
  });
  const silent = $derived(!starting && seconds > 6 && loudest < 0.004);

  let stopping = $state(false);
  async function stop() {
    stopping = true;
    await api.stopRecording(recording.id, true).catch(showError);
  }
  async function cancel() {
    const ok = await confirm({ title: t("record.cancelTitle"), text: t("record.cancelText"), confirm: t("record.cancelConfirm"), danger: true });
    if (!ok) return;
    stopping = true;
    await api.stopRecording(recording.id, false).catch(showError);
    app.go("home");
  }

  let box = $state<HTMLElement>();
  /** Прокручиваем за текстом, пока читатель сам не ушёл вверх. */
  let pinned = true;
  $effect(() => {
    job?.lines.length;
    if (pinned && box) box.scrollTop = box.scrollHeight;
  });
</script>

<div class="record">
  <div class="panel">
    <div class="status">
      <span class="dot" class:idle={starting}></span>
      <span class="timer">{fmtTime(seconds)}</span>
      <span class="label muted">{starting ? job?.title || t("live.starting") : t("record.listening")}</span>
    </div>
    <div class="meter" aria-hidden="true"><span style="width:{Math.min(100, Math.round(Math.sqrt(peak) * 100))}%"></span></div>
    <div class="buttons">
      <button class="primary stop" onclick={stop} disabled={stopping}>
        {#if stopping}<span class="spinner"></span>{:else}<Icon name="stop" size={13} />{/if}
        {t("record.stop")}
      </button>
      <button class="ghost" onclick={cancel} disabled={stopping}>{t("record.cancel")}</button>
    </div>
  </div>

  {#if silent}
    <div class="warn"><Icon name="alert" /> <span>{t("record.silent")}</span></div>
  {/if}

  <div class="text" bind:this={box} onscroll={() => box && (pinned = box.scrollHeight - box.scrollTop - box.clientHeight < 40)}>
    {#each job?.lines ?? [] as line, i (i)}
      <p><span class="ts">{fmtTime(line.start)}</span><span>{line.text}</span></p>
    {:else}
      <p class="empty faint">{t("record.empty")}</p>
    {/each}
    {#if !starting}<span class="caret"></span>{/if}
  </div>
  <p class="note faint">{t("record.hint")}</p>
</div>

<style>
  .record { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .panel { padding: 18px 28px 16px; border-bottom: 1px solid var(--border); background: var(--bg-card); display: flex; flex-direction: column; gap: 12px; }
  .status { display: flex; align-items: center; gap: 10px; }
  .dot { width: 12px; height: 12px; border-radius: 50%; background: var(--danger); box-shadow: 0 0 0 4px color-mix(in srgb, var(--danger) 18%, transparent); animation: pulse 1.4s ease-in-out infinite; }
  .dot.idle { background: var(--fg-faint); box-shadow: none; animation: none; }
  .timer { font-size: 26px; font-weight: 650; font-variant-numeric: tabular-nums; letter-spacing: -0.01em; }
  .label { font-size: var(--fs-sm); }
  .meter { height: 5px; border-radius: 3px; background: var(--bg-active); overflow: hidden; max-width: 520px; }
  .meter span { display: block; height: 100%; border-radius: 3px; background: var(--ok); transition: width 0.12s linear; }
  .buttons { display: flex; align-items: center; gap: 8px; }
  .stop { display: inline-flex; align-items: center; gap: 7px; padding: 6px 16px; font-weight: 550; }
  .stop .spinner { border-top-color: var(--accent-fg); }

  .warn {
    display: flex; align-items: flex-start; gap: 8px; padding: 8px 28px; font-size: var(--fs-sm); color: var(--warn);
    background: color-mix(in srgb, var(--warn) 10%, transparent); border-bottom: 1px solid var(--border);
  }
  .warn :global(svg) { flex-shrink: 0; margin-top: 2px; }

  .text { flex: 1; overflow-y: auto; padding: 18px 28px 8px; user-select: text; -webkit-user-select: text; }
  p { display: flex; gap: 8px; margin: 0 0 6px; font-size: 14.5px; line-height: 1.6; max-width: 820px; animation: appear 0.25s ease-out; }
  .ts { width: 46px; flex-shrink: 0; text-align: right; color: var(--fg-faint); font-size: var(--fs-xs); font-variant-numeric: tabular-nums; line-height: 2.1; }
  .empty { padding-left: 54px; animation: none; }
  .caret { display: inline-block; width: 7px; height: 15px; margin-left: 54px; border-radius: 1px; background: var(--danger); animation: blink 1s steps(2) infinite; }
  .note { margin: 0; padding: 8px 28px 12px; font-size: var(--fs-sm); text-align: center; }
  @keyframes appear { from { opacity: 0; transform: translateY(4px); } }
  @keyframes blink { 50% { opacity: 0; } }
  @keyframes pulse { 50% { box-shadow: 0 0 0 8px transparent; } }
  @media (prefers-reduced-motion: reduce) { p, .caret, .dot { animation: none; } }
</style>
