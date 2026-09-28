<script lang="ts">
  import { fmtTime, type LiveState } from "./api";
  import { t, type Key } from "./i18n.svelte";
  import { app } from "./state.svelte";
  import Progress from "./ui/Progress.svelte";

  /** Расшифровка в реальном времени: этапы работы и текст по мере распознавания. */
  let { job }: { job?: LiveState } = $props();

  // Крупные шаги; этапы ядра раскладываются по ним.
  const phases: { title: Key; stages: string[]; on: boolean }[] = $derived([
    { title: "live.prepare", stages: ["load", "prepare", "denoise"], on: true },
    { title: "live.recognize", stages: ["recognize"], on: true },
    { title: "live.speakers", stages: ["diarize", "identify"], on: app.settings?.diar_model !== "off" },
    { title: "live.polish", stages: ["terms", "polish", "enrich"], on: app.settings?.llm_enabled ?? false },
    { title: "live.save", stages: ["condense", "protocol", "export"], on: true },
  ].filter((p) => p.on) as { title: Key; stages: string[]; on: boolean }[]);
  const at = $derived(Math.max(0, phases.findIndex((p) => p.stages.includes(job?.stage ?? ""))));

  let box = $state<HTMLElement>();
  /** Прокручиваем за текстом, пока читатель сам не ушёл вверх. */
  let pinned = true;
  $effect(() => {
    job?.lines.length;
    if (pinned && box) box.scrollTop = box.scrollHeight;
  });
</script>

<div class="live">
  <div class="steps">
    <ol>
      {#each phases as p, i (p.title)}
        <li class:done={i < at} class:now={i === at}><span class="mark"></span>{t(p.title)}</li>
      {/each}
    </ol>
    <div class="stage">
      <span>{job?.title || t("live.starting")}{job && job.progress > 0 ? ` · ${Math.round(job.progress * 100)}%` : ""}</span>
      <Progress value={job?.progress ?? 0} />
    </div>
  </div>

  <div class="text" bind:this={box} onscroll={() => box && (pinned = box.scrollHeight - box.scrollTop - box.clientHeight < 40)}>
    {#each job?.lines ?? [] as line, i (i)}
      <p><span class="ts">{fmtTime(line.start)}</span><span>{line.text}</span></p>
    {:else}
      <p class="empty faint">{t("live.empty")}</p>
    {/each}
    {#if job?.stage === "recognize"}<span class="caret"></span>{/if}
  </div>
  <p class="note faint">{t("rec.background")}</p>
</div>

<style>
  .live { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .steps { padding: 16px 28px 14px; border-bottom: 1px solid var(--border); background: var(--bg-card); }
  ol { list-style: none; display: flex; flex-wrap: wrap; gap: 6px 22px; margin: 0 0 12px; padding: 0; }
  li { display: flex; align-items: center; gap: 7px; color: var(--fg-faint); font-size: var(--fs-sm); }
  li.done { color: var(--fg-muted); }
  li.now { color: var(--fg); font-weight: 600; }
  .mark { width: 8px; height: 8px; border-radius: 50%; background: var(--border-strong); }
  .done .mark { background: var(--ok); }
  .now .mark { background: var(--accent); box-shadow: 0 0 0 4px var(--bg-selected); animation: pulse 1.4s ease-in-out infinite; }
  .stage { display: flex; flex-direction: column; gap: 6px; font-size: var(--fs-sm); color: var(--fg-muted); max-width: 520px; }

  .text { flex: 1; overflow-y: auto; padding: 18px 28px 8px; user-select: text; -webkit-user-select: text; }
  p { display: flex; gap: 8px; margin: 0 0 6px; font-size: 14.5px; line-height: 1.6; max-width: 820px; animation: appear 0.25s ease-out; }
  .ts { width: 46px; flex-shrink: 0; text-align: right; color: var(--fg-faint); font-size: var(--fs-xs); font-variant-numeric: tabular-nums; line-height: 2.1; }
  .empty { padding-left: 54px; animation: none; }
  .caret { display: inline-block; width: 7px; height: 15px; margin-left: 54px; border-radius: 1px; background: var(--accent); animation: blink 1s steps(2) infinite; }
  .note { margin: 0; padding: 8px 28px 12px; font-size: var(--fs-sm); text-align: center; }
  @keyframes appear { from { opacity: 0; transform: translateY(4px); } }
  @keyframes blink { 50% { opacity: 0; } }
  @keyframes pulse { 50% { box-shadow: 0 0 0 7px transparent; } }
  @media (prefers-reduced-motion: reduce) { p, .caret, .now .mark { animation: none; } }
</style>
