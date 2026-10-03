<script lang="ts">
  import icon from "./assets/icon.png";
  import Icon from "./Icon.svelte";
  import { api, type Stats } from "./api";
  import { fmtDuration, fmtSize, t, tn, type Key } from "./i18n.svelte";
  import { models, ready, starter } from "./models.svelte";
  import { startRecording } from "./recordings";
  import { app, type SettingsSection } from "./state.svelte";
  import Progress from "./ui/Progress.svelte";

  let { onadd }: { onadd: () => void } = $props();

  let stats = $state<Stats | null>(null);
  $effect(() => {
    // Счётчики обновляются вместе с библиотекой и справочниками.
    app.recordings.length;
    app.pending;
    api.stats().then((s) => (stats = s), () => {});
  });

  // ---------- первый запуск: модели ----------
  const set = $derived(starter());
  const need = $derived(set.filter((m) => !m.installed));
  const total = $derived(set.reduce((a, m) => a + m.size_mb, 0));
  const left = $derived(need.reduce((a, m) => a + m.size_mb, 0));
  const loading = $derived(need.some((m) => models.progress(m.name) !== undefined));
  /** Доля скачанного по всем нужным моделям. */
  const done = $derived(
    total ? (total - left + need.reduce((a, m) => a + (models.progress(m.name) ?? 0) * m.size_mb, 0)) / total : 1,
  );

  // ---------- советы ----------
  type Tip = { icon: string; title: Key; text: Key; action?: Key; run?: () => void; done?: boolean };
  const settings = (s: SettingsSection) => () => app.go("settings", s);
  const tips: Tip[] = $derived([
    { icon: "book", title: "tips.dictionary", text: "tips.dictionaryText", action: "tips.dictionaryAction", run: () => app.go("dictionary"), done: (stats?.terms ?? 0) > 0 },
    { icon: "people", title: "tips.voices", text: "tips.voicesText", action: "tips.voicesAction", run: () => app.go("people"), done: (stats?.voices ?? 0) > 0 },
    { icon: "sparkle", title: "tips.llm", text: "tips.llmText", action: "tips.llmAction", run: settings("llm"), done: app.settings?.llm_enabled },
    { icon: "inbox", title: "tips.watch", text: "tips.watchText", action: "tips.watchAction", run: settings("watch"), done: (stats?.rules ?? 0) > 0 },
    { icon: "doc", title: "tips.templates", text: "tips.templatesText", action: "tips.templatesAction", run: settings("documents") },
    { icon: "volume", title: "tips.audio", text: "tips.audioText", action: "tips.audioAction", run: settings("audio"), done: app.settings?.denoise },
  ]);

  const keys: { keys: string[]; text: Key }[] = [
    { keys: ["Space"], text: "keys.play" },
    { keys: ["⌘", "K"], text: "keys.search" },
    { keys: ["⌘", "R"], text: "keys.record" },
    { keys: ["⌘", "O"], text: "keys.add" },
    { keys: ["⌘", ","], text: "keys.settings" },
    { keys: ["↑", "↓"], text: "keys.list" },
  ];
  const mac = navigator.platform.toLowerCase().includes("mac");
  const key = (k: string) => (k === "Space" ? t("keys.space") : k === "⌘" && !mac ? "Ctrl" : k);
</script>

<div class="welcome">
  <div class="column">
    <header>
      <img src={icon} alt="" width="56" height="56" />
      <div>
        <h1>{t(app.recordings.length ? "app.name" : "welcome.title")}</h1>
        <p class="muted">{t("welcome.lead")}</p>
      </div>
    </header>

    {#if !ready()}
      <section class="setup">
        <h2>{t("welcome.setup")}</h2>
        <p class="muted">{t("welcome.setupText", { size: fmtSize(total) })}</p>
        {#if loading}
          <div class="loading"><Progress value={done} /><span class="muted">{Math.round(done * 100)}%</span></div>
        {:else}
          <button class="primary big" onclick={() => models.download(need.map((m) => m.name))}>
            <Icon name="download" /> {t("welcome.download", { size: fmtSize(left) })}
          </button>
        {/if}
        <ul class="models">
          {#each set as m (m.name)}
            <li class:ok={m.installed}><Icon name={m.installed ? "check" : "download"} size={12} /> {m.title} <span class="faint">{m.about}</span></li>
          {/each}
        </ul>
        <p class="faint small">{t("welcome.modelsNote")}</p>
      </section>
    {:else}
      <div class="start">
        <button class="card record" onclick={startRecording}>
          <span class="ico"><Icon name="mic" size={22} /></span>
          <span class="big-text">{t("welcome.record")}</span>
          <span class="muted">{t("welcome.recordHint")}</span>
        </button>
        <button class="card drop" onclick={onadd}>
          <span class="ico plain"><Icon name="wave" size={22} /></span>
          <span class="big-text">{t("welcome.drop")}</span>
          <span class="muted">{t("welcome.dropHint")}</span>
        </button>
      </div>
    {/if}

    {#if stats && stats.recordings > 0}
      <div class="stats">
        <div><b>{stats.recordings}</b><span>{tn("stats.recordings", stats.recordings)}</span></div>
        <div><b>{fmtDuration(stats.seconds)}</b><span>{t("stats.audio")}</span></div>
        <div><b>{stats.terms}</b><span>{tn("stats.terms", stats.terms)}</span></div>
        <div><b>{stats.people}</b><span>{tn("stats.people", stats.people)}</span></div>
      </div>
    {/if}

    <section>
      <h2>{t("tips.title")}</h2>
      <div class="tips">
        {#each tips as tip (tip.title)}
          <div class="tip" class:done={tip.done}>
            <span class="ico"><Icon name={tip.done ? "check" : tip.icon} size={16} /></span>
            <div>
              <h3>{t(tip.title)}</h3>
              <p class="muted">{t(tip.text)}</p>
              {#if tip.action && tip.run}<button class="link" onclick={tip.run}>{t(tip.action)} →</button>{/if}
            </div>
          </div>
        {/each}
      </div>
    </section>

    <section class="facts">
      <div class="fact"><Icon name="lock" size={15} /><p><b>{t("facts.private")}</b> {t("facts.privateText")}</p></div>
      <div class="fact"><Icon name="clock" size={15} /><p><b>{t("facts.background")}</b> {t("facts.backgroundText")}</p></div>
      <div class="fact"><Icon name="more" size={15} /><p><b>{t("facts.menu")}</b> {t("facts.menuText")}</p></div>
    </section>

    <section>
      <h2>{t("keys.title")}</h2>
      <div class="keys">
        {#each keys as k (k.text)}
          <div><span class="combo">{#each k.keys as c}<kbd>{key(c)}</kbd>{/each}</span><span class="muted">{t(k.text)}</span></div>
        {/each}
      </div>
    </section>
  </div>
</div>

<style>
  .welcome { flex: 1; overflow-y: auto; min-height: 0; }
  .column { max-width: 780px; margin: 0 auto; padding: 48px 32px 48px; display: flex; flex-direction: column; gap: 26px; }
  header { display: flex; align-items: center; gap: 16px; }
  header img { border-radius: 13px; }
  header p { margin: 3px 0 0; }
  h2 { margin: 0 0 10px; }
  section p { margin: 0; }

  .setup { border: 1px solid var(--border); border-radius: var(--r-xl); background: var(--bg-card); padding: 20px 22px; }
  .setup h2 { font-size: var(--fs-lg); margin-bottom: 4px; }
  .big { display: inline-flex; align-items: center; gap: 8px; padding: 8px 18px; margin-top: 14px; font-weight: 550; }
  .loading { display: flex; align-items: center; gap: 12px; margin-top: 18px; font-variant-numeric: tabular-nums; }
  .loading :global(.bar) { flex: 1; }
  .models { list-style: none; padding: 0; margin: 16px 0 10px; display: flex; flex-direction: column; gap: 4px; }
  .models li { display: flex; align-items: center; gap: 8px; color: var(--fg-muted); }
  .models li.ok { color: var(--fg); }
  .models li.ok :global(svg) { color: var(--ok); }
  .small { font-size: var(--fs-sm); }

  .start { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .card {
    display: flex; flex-direction: column; align-items: center; gap: 8px; padding: 26px 20px; white-space: normal;
    border-radius: var(--r-xl); color: var(--fg-muted); transition: border-color 0.15s, background 0.15s, color 0.15s;
  }
  .card .ico {
    width: 44px; height: 44px; border-radius: 50%; display: grid; place-items: center; margin-bottom: 4px;
    background: var(--danger); color: #fff;
  }
  .card .ico.plain { background: var(--bg-selected); color: var(--accent); }
  .record { border: 1px solid var(--border); background: var(--bg-card); }
  .record:hover { border-color: var(--danger); background: color-mix(in srgb, var(--danger) 6%, var(--bg-card)); }
  .drop { border: 1.5px dashed var(--border-strong); background: transparent; }
  .drop:hover { border-color: var(--accent); color: var(--accent); background: var(--bg-selected); }
  .big-text { font-size: 16px; font-weight: 600; color: var(--fg); }

  .stats { display: grid; grid-template-columns: repeat(4, 1fr); gap: 10px; }
  .stats div { border: 1px solid var(--border); border-radius: var(--r-lg); padding: 10px 14px; display: flex; flex-direction: column; }
  .stats b { font-size: var(--fs-lg); font-weight: 650; font-variant-numeric: tabular-nums; }
  .stats span { color: var(--fg-muted); font-size: var(--fs-sm); }

  .tips { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }
  .tip { display: flex; gap: 12px; border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--bg-card); padding: 13px 14px; }
  .tip h3 { margin: 0 0 2px; font-size: var(--fs-md); font-weight: 600; }
  .tip p { font-size: var(--fs-sm); }
  .tip .link { margin-top: 6px; font-size: var(--fs-sm); }
  .ico {
    width: 30px; height: 30px; border-radius: var(--r-md); display: grid; place-items: center; flex-shrink: 0;
    background: var(--bg-selected); color: var(--accent);
  }
  .done .ico { background: color-mix(in srgb, var(--ok) 14%, transparent); color: var(--ok); }

  .facts { display: flex; flex-direction: column; gap: 8px; }
  .fact { display: flex; gap: 10px; align-items: flex-start; color: var(--fg-muted); }
  .fact :global(svg) { margin-top: 2px; flex-shrink: 0; }
  .fact b { color: var(--fg); font-weight: 550; }

  .keys { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 6px 24px; }
  .keys div { display: flex; align-items: center; gap: 10px; }
  .combo { display: inline-flex; gap: 3px; min-width: 74px; }
  @media (max-width: 1020px) { .tips, .keys, .start { grid-template-columns: 1fr; } .stats { grid-template-columns: repeat(2, 1fr); } }
</style>
