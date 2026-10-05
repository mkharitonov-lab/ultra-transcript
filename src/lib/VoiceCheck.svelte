<script lang="ts">
  import { tick } from "svelte";
  import Icon from "./Icon.svelte";
  import { fmtTime, showError, type Transcript, type Utterance } from "./api";
  import { t } from "./i18n.svelte";

  /**
   * Проверка на слух перед тем, как голос спикера пойдёт в профиль человека: несколько самых
   * длинных отрезков спикера из разных мест записи — по ним отпечаток и считается. Если
   * диаризация склеила двух людей, это слышно здесь.
   * `onanswer`: true — запомнить голос, false — только подписать имя, null — отмена.
   */
  let { transcript, speaker, name, src, onanswer }: {
    transcript: Transcript;
    speaker: string;
    name: string;
    src: string;
    onanswer: (remember: boolean | null) => void;
  } = $props();

  /** Сколько отрезков дать послушать и насколько длинных, секунд. */
  const COUNT = 4;
  const SECONDS = 15;
  /** Запас перед началом: таймкод первого слова бывает чуть поздним. */
  const LEAD_IN = 0.25;

  type Fragment = { start: number; end: number; text: string };

  const fragments = $derived.by(() => {
    const runs: Utterance[][] = [];
    for (const u of transcript.utterances) {
      const last = runs.at(-1);
      if (last && last[0].speaker === u.speaker) last.push(u);
      else runs.push([u]);
    }
    const span = (r: Utterance[]) => r[r.length - 1].end - r[0].start;
    return runs
      .filter((r) => r[0].speaker === speaker)
      .sort((a, b) => span(b) - span(a))
      .slice(0, COUNT)
      .map((r): Fragment => {
        const start = Math.max(0, r[0].start - LEAD_IN);
        const end = Math.min(r[r.length - 1].end, start + SECONDS);
        return { start, end, text: r.filter((u) => u.start < end).map((u) => u.clean || u.text).join(" ") };
      })
      .sort((a, b) => a.start - b.start);
  });

  let player = $state<HTMLAudioElement>();
  let playing = $state<number | null>(null);
  /** Подтвердить можно, только послушав хотя бы один отрезок. */
  let heard = $state(false);
  let box = $state<HTMLElement>();
  let stopAt = 0;

  $effect(() => {
    tick().then(() => box?.querySelector<HTMLElement>("button.play")?.focus());
  });

  function toggle(i: number) {
    if (!player) return;
    if (playing === i) return player.pause();
    const f = fragments[i];
    stopAt = f.end;
    // Пока файл не открылся, перемотка не сработает — перематываем, когда откроется.
    if (player.readyState < 1) player.addEventListener("loadedmetadata", () => player && (player.currentTime = f.start), { once: true });
    else player.currentTime = f.start;
    playing = i;
    heard = true;
    player.play().catch((e) => {
      if (e?.name === "AbortError") return;
      playing = null;
      showError(e);
    });
  }

  function answer(v: boolean | null) {
    player?.pause();
    onanswer(v);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      answer(null);
    }
  }
</script>

<svelte:window onkeydowncapture={onkeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" onmousedown={(e) => { if (e.target === e.currentTarget) answer(null); }}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="voice-check-title" bind:this={box}>
    <audio bind:this={player} {src} preload="auto"
      ontimeupdate={() => { if (player && playing !== null && player.currentTime >= stopAt) player.pause(); }}
      onpause={() => (playing = null)}
      onended={() => (playing = null)}></audio>
    <h2 id="voice-check-title">{t("check.title", { name })}</h2>
    <p class="muted">{t("check.text", { name })}</p>
    <div class="fragments">
      {#each fragments as f, i (f.start)}
        <div class="fragment">
          <button class="play" onclick={() => toggle(i)} aria-label={t(playing === i ? "player.pause" : "voices.listen")}>
            <Icon name={playing === i ? "pause" : "play"} size={10} />
          </button>
          <span class="ts faint">{fmtTime(f.start)}</span>
          <span class="quote muted" title={f.text}>«{f.text}»</span>
        </div>
      {:else}
        <p class="faint">{t("check.nothing")}</p>
      {/each}
    </div>
    <div class="buttons">
      <button onclick={() => answer(null)}>{t("common.cancel")}</button>
      <button onclick={() => answer(false)} title={t("check.nameOnlyHint")}>{t("check.nameOnly")}</button>
      <button class="primary" onclick={() => answer(true)} disabled={!heard} title={heard ? "" : t("check.listenFirst")}>{t("check.remember")}</button>
    </div>
    {#if !heard && fragments.length}<p class="faint hint">{t("check.listenFirst")}</p>{/if}
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; z-index: 650; display: grid; place-items: center; padding: 24px;
    background: var(--scrim); animation: fade 0.14s ease-out;
  }
  .dialog {
    width: min(500px, 100%); padding: 20px; border-radius: var(--r-xl); background: var(--bg-elevated);
    border: 1px solid var(--border); box-shadow: var(--shadow-lg); animation: rise 0.18s var(--ease);
  }
  h2 { font-size: var(--fs-lg); margin: 0; }
  p { margin: 6px 0 0; }
  .fragments { margin-top: 12px; border-top: 1px solid var(--border); }
  .fragment { display: flex; align-items: center; gap: 10px; padding: 7px 0; border-bottom: 1px solid var(--border); min-width: 0; }
  .play {
    width: 24px; height: 24px; flex-shrink: 0; border-radius: 50%; padding: 0; display: grid; place-items: center;
    background: var(--accent); color: var(--accent-fg); border: none;
  }
  .ts { font-size: var(--fs-xs); font-variant-numeric: tabular-nums; flex-shrink: 0; }
  .quote { font-size: var(--fs-sm); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; margin-top: 20px; flex-wrap: wrap; }
  .buttons button { padding: 6px 14px; }
  .hint { font-size: var(--fs-xs); text-align: right; }
  @keyframes fade { from { opacity: 0; } }
  @keyframes rise { from { opacity: 0; transform: translateY(8px) scale(0.98); } }
  @media (prefers-reduced-motion: reduce) { .scrim, .dialog { animation: none; } }
</style>
