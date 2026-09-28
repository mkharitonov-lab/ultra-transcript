<script lang="ts">
  import { onMount, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import {
    api, audioUrl, confirmDialog, fmtTime, recordingDir, showError, speakerColor,
    type AppInfo, type AsrModel, type Person, type Recording, type Transcript,
  } from "./api";

  let {
    recording, info, progress, onchange, onremove,
  }: {
    recording: Recording; info: AppInfo; progress?: { stage: string; progress: number };
    onchange: () => void; onremove: () => void;
  } = $props();

  let t = $state<Transcript | null>(null);
  let tab = $state<"transcript" | "protocol">("transcript");
  let mode = $state<"clean" | "text">("clean");
  let people = $state<Person[]>([]);
  let picking = $state<string | null>(null);
  let query = $state("");
  let audio = $state<HTMLAudioElement>();
  let time = $state(0);
  let paused = $state(true);
  let rate = $state(1);
  let follow = $state(true);
  let list = $state<HTMLElement>();
  let redoMenu = $state(false);

  const asrOptions: [AsrModel, string][] = [["gigaam", "GigaAM v3"], ["whisper-turbo", "Whisper large-v3-turbo"]];
  const installed = (id: AsrModel) => info.models.some((m) => m.asr === id && m.installed);

  async function redo(asr: AsrModel) {
    redoMenu = false;
    if (t && !(await confirmDialog("Расшифровать запись заново? Ручные правки текста и протокол будут заменены."))) return;
    await api.retry(recording.id, asr).catch(showError);
    onchange();
  }

  const dir = $derived(recordingDir(info.data_dir, recording.id));
  const active = $derived(t ? t.utterances.findLastIndex((u) => u.start <= time + 0.05) : -1);
  const filtered = $derived(people.filter((p) => !query || p.name.toLowerCase().includes(query.toLowerCase())));

  async function load() {
    try {
      t = await api.transcript(recording.id);
    } catch {
      t = null;
    }
  }

  // Перечитываем расшифровку, когда фоновая задача по этой записи завершилась.
  let lastStatus = "";
  $effect(() => {
    const s = recording.status;
    if (lastStatus && s !== lastStatus && s === "done" && !document.activeElement?.closest?.(".text")) load();
    lastStatus = s;
  });

  onMount(() => {
    load();
    api.people().then((p) => (people = p));
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement;
      if (e.code === "Space" && !el.isContentEditable && !["INPUT", "TEXTAREA"].includes(el.tagName)) {
        e.preventDefault();
        if (audio) paused ? audio.play() : audio.pause();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  $effect(() => {
    if (follow && !paused && active >= 0 && list) {
      list.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }
  });

  function seek(sec: number) {
    if (!audio) return;
    audio.currentTime = sec;
    audio.play();
  }

  async function save() {
    if (!t) return;
    await api.saveTranscript($state.snapshot(t) as Transcript).catch(showError);
    onchange();
  }

  function editText(i: number, el: HTMLElement) {
    if (!t) return;
    const value = el.innerText.trim();
    const u = t.utterances[i];
    if (u[mode] === value) return;
    u[mode] = value;
    save();
  }

  async function renameTitle(e: Event) {
    const v = (e.target as HTMLInputElement).value.trim();
    if (t && v && v !== t.title) {
      t.title = v;
      await save();
    }
  }

  async function assign(speaker: string, personId: number | null, name = "") {
    picking = null;
    query = "";
    try {
      t = await api.assignSpeaker(recording.id, speaker, personId, name);
      people = await api.people();
      onchange();
    } catch (e) {
      showError(e);
    }
  }

  async function protocol() {
    await api.makeProtocol(recording.id);
    onchange();
    tab = "protocol";
  }

  function label(key: string) {
    const s = key.replaceAll("_", " ");
    return s[0].toUpperCase() + s.slice(1);
  }

  const asList = (v: unknown) => (Array.isArray(v) ? v : []);
  const asText = (v: unknown) =>
    typeof v === "string" ? v : Array.isArray(v) ? v.join(", ") : v == null ? "" : JSON.stringify(v);

  async function openPicker(id: string) {
    picking = picking === id ? null : id;
    query = "";
    await tick();
    (document.querySelector(".picker input") as HTMLInputElement | null)?.focus();
  }
</script>

<div class="view">
  <header>
    <div class="head-row">
      {#if t}
        <input class="title" value={t.title} onchange={renameTitle} spellcheck="false" />
      {:else}
        <h1 class="title-static">{recording.title}</h1>
      {/if}
      <div class="actions">
        {#if t}
          <div class="seg">
            <button class:on={tab === "transcript"} onclick={() => (tab = "transcript")}>Расшифровка</button>
            <button class:on={tab === "protocol"} onclick={() => (tab = "protocol")}>Протокол</button>
          </div>
          <button class="ghost" title="Открыть .docx" onclick={() => api.reveal(`${dir}/${tab === "protocol" && t?.protocol ? "protocol" : "transcript"}.docx`, true)}><Icon name="doc" /></button>
          <button class="ghost" title="Показать файлы" onclick={() => api.reveal(`${dir}/transcript.docx`)}><Icon name="reveal" /></button>
        {/if}
        <div class="redo">
          <button class="ghost" title="Расшифровать заново" onclick={() => (redoMenu = !redoMenu)}
            disabled={recording.status === "processing" || recording.status === "queued"}><Icon name="refresh" /></button>
          {#if redoMenu}
            <div class="menu">
              <div class="menu-title faint">Расшифровать заново моделью</div>
              {#each asrOptions as [id, name]}
                <button onclick={() => redo(id)} disabled={!installed(id)}>
                  {name}{#if t?.asr_model === name}<span class="faint"> · текущая</span>{/if}{#if !installed(id)}<span class="faint"> · не скачана</span>{/if}
                </button>
              {/each}
            </div>
          {/if}
        </div>
        <button class="ghost danger" title="Удалить" onclick={onremove}><Icon name="trash" /></button>
      </div>
    </div>
    <div class="meta muted">
      {recording.created_at} · {fmtTime(t?.duration ?? recording.duration)}
      {#if t} · {t.speakers.length} {t.speakers.length === 1 ? "спикер" : "спикера"}{#if t.asr_model} · {t.asr_model}{/if}{/if}
      · <span class="src" title={recording.source}>{recording.source.split(/[\\/]/).pop()}</span>
    </div>

    {#if t && tab === "transcript"}
      <div class="speakers">
        {#each t.speakers as s (s.id)}
          <div class="chip-wrap">
            <button class="chip" onclick={() => openPicker(s.id)}>
              <span class="dot" style="background:{speakerColor(s.id)}"></span>
              {s.name}
              {#if s.similarity}<span class="sim" title="Опознан по голосу">{Math.round(s.similarity * 100)}%</span>{/if}
              {#if !s.person_id}<span class="faint">· кто это?</span>{/if}
            </button>
            {#if picking === s.id}
              <div class="picker">
                <input type="text" placeholder="Имя или поиск…" bind:value={query}
                  onkeydown={(e) => {
                    if (e.key === "Escape") picking = null;
                    if (e.key === "Enter" && query.trim()) {
                      const exact = people.find((p) => p.name.toLowerCase() === query.trim().toLowerCase());
                      assign(s.id, exact?.id ?? null, exact ? "" : query);
                    }
                  }} />
                <div class="options">
                  {#each filtered.slice(0, 8) as p (p.id)}
                    <button onclick={() => assign(s.id, p.id)}>
                      {p.name} {#if p.role}<span class="faint">· {p.role}</span>{/if}
                      {#if p.voiceprints}<span class="faint">· голос</span>{/if}
                    </button>
                  {/each}
                  {#if query.trim() && !people.some((p) => p.name.toLowerCase() === query.trim().toLowerCase())}
                    <button onclick={() => assign(s.id, null, query)}><Icon name="plus" size={12} /> Добавить «{query.trim()}» в справочник</button>
                  {/if}
                  {#if s.person_id}<button class="danger" onclick={() => assign(s.id, null, "")}>Сбросить</button>{/if}
                </div>
                <p class="faint hint">Голос запомнится — в следующих записях этот человек определится сам.</p>
              </div>
            {/if}
          </div>
        {/each}
        <div class="spacer"></div>
        <div class="seg small">
          <button class:on={mode === "clean"} onclick={() => (mode = "clean")} title="Без слов-паразитов">Очищенный</button>
          <button class:on={mode === "text"} onclick={() => (mode = "text")} title="Как сказано">Дословный</button>
        </div>
      </div>
    {/if}
  </header>

  {#if recording.status === "queued" && t}
    <div class="banner"><Icon name="clock" /> В очереди на расшифровку</div>
  {/if}
  {#if recording.status === "error" && t}
    <div class="banner warn" title={recording.error}><Icon name="alert" /> {recording.error}</div>
  {/if}
  {#if recording.status === "processing" && t}
    <div class="banner">
      <span class="spinner"></span>{progress?.stage || "Обработка"}{progress && progress.progress > 0 ? ` · ${Math.round(progress.progress * 100)}%` : ""}
    </div>
  {/if}
  {#if recording.status === "done" && recording.error}
    <div class="banner warn" title={recording.error}><Icon name="alert" /> {recording.error.split("\n")[0]}</div>
  {/if}

  {#if !t}
    <div class="state">
      {#if recording.status === "error"}
        <Icon name="alert" size={32} />
        <p>{recording.error}</p>
        <button onclick={() => api.retry(recording.id).then(onchange)}><Icon name="refresh" /> Повторить</button>
      {:else if recording.status === "queued"}
        <Icon name="clock" size={32} />
        <p class="muted">В очереди</p>
      {:else}
        <p class="stage">{progress?.stage || "Подготовка"}</p>
        <div class="bar"><span style="width:{Math.round((progress?.progress ?? 0) * 100)}%"></span></div>
        <p class="faint">Можно закрыть окно — обработка продолжится в фоне.</p>
      {/if}
    </div>
  {:else if tab === "transcript"}
    <div class="list" bind:this={list}>
      {#each t.utterances as u, i (u.id)}
        {@const prev = t.utterances[i - 1]}
        <div class="utt" class:active={i === active && !paused} data-i={i}>
          {#if !prev || prev.speaker !== u.speaker}
            <div class="who">
              <span class="name" style="color:{speakerColor(u.speaker)}">{t.speakers.find((s) => s.id === u.speaker)?.name ?? u.speaker}</span>
            </div>
          {/if}
          <div class="line">
            <button class="ts" onclick={() => seek(u.start)}>{fmtTime(u.start)}</button>
            {#key mode}
              <div class="text" contenteditable="plaintext-only" spellcheck="false"
                onblur={(e) => editText(i, e.currentTarget)}>{u[mode] || u.text}</div>
            {/key}
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <div class="protocol">
      {#if t.protocol}
        {#each Object.entries(t.protocol) as [k, v]}
          <section>
            <h2>{label(k)}</h2>
            {#if Array.isArray(v) && v.length && typeof v[0] === "object"}
              <table>
                <thead><tr>{#each Object.keys(v[0] as object) as c}<th>{label(c)}</th>{/each}</tr></thead>
                <tbody>
                  {#each v as row}<tr>{#each Object.values(row as object) as cell}<td>{asText(cell)}</td>{/each}</tr>{/each}
                </tbody>
              </table>
            {:else if Array.isArray(v)}
              <ul>{#each asList(v) as item}<li>{asText(item)}</li>{/each}</ul>
            {:else}
              <p>{asText(v) || "—"}</p>
            {/if}
          </section>
        {/each}
        <div class="proto-actions">
          <button onclick={() => api.reveal(`${dir}/protocol.docx`, true)}><Icon name="doc" /> Открыть .docx</button>
          <button class="ghost" onclick={protocol} disabled={recording.status === "processing"}><Icon name="refresh" /> Составить заново</button>
        </div>
      {:else}
        <div class="state">
          <Icon name="doc" size={32} />
          <p class="muted">Сжатый протокол договорённостей по шаблону .docx</p>
          <button class="primary" onclick={protocol} disabled={recording.status === "processing"}>Составить протокол</button>
        </div>
      {/if}
    </div>
  {/if}

  {#if t}
    <footer class="player">
      <audio bind:this={audio} src={audioUrl(info.data_dir, recording.id)} bind:currentTime={time} bind:paused bind:playbackRate={rate} preload="metadata"></audio>
      <button class="ghost" onclick={() => audio && (audio.currentTime -= 5)} title="−5 с"><Icon name="back" /></button>
      <button class="play" onclick={() => (paused ? audio?.play() : audio?.pause())}><Icon name={paused ? "play" : "pause"} size={16} /></button>
      <button class="ghost" onclick={() => audio && (audio.currentTime += 5)} title="+5 с"><Icon name="fwd" /></button>
      <span class="time">{fmtTime(time)}</span>
      <input class="scrub" type="range" min="0" max={t.duration} step="0.1" bind:value={time}
        oninput={(e) => audio && (audio.currentTime = +e.currentTarget.value)} />
      <span class="time faint">{fmtTime(t.duration)}</span>
      <select bind:value={rate} class="rate">
        {#each [0.75, 1, 1.25, 1.5, 1.75, 2] as r}<option value={r}>{r}×</option>{/each}
      </select>
      <label class="follow faint"><input type="checkbox" bind:checked={follow} /> следить</label>
    </footer>
  {/if}
</div>

<style>
  .view { display: flex; flex-direction: column; height: 100vh; min-height: 0; }
  header { padding: 34px 28px 10px; border-bottom: 1px solid var(--border); position: relative; z-index: 6; }
  .head-row { display: flex; align-items: center; gap: 12px; }
  .title, .title-static { flex: 1; font-size: 20px; font-weight: 650; border: none !important; background: transparent !important; padding: 2px 0 !important; box-shadow: none !important; margin: 0; }
  .actions { display: flex; align-items: center; gap: 4px; }
  .redo { position: relative; }
  .menu { position: absolute; right: 0; top: 32px; width: 270px; background: var(--bg); border: 1px solid var(--border); border-radius: 10px; box-shadow: var(--shadow); padding: 6px; z-index: 20; display: flex; flex-direction: column; }
  .menu-title { font-size: 11px; padding: 4px 8px; }
  .menu button { border: none; background: transparent; text-align: left; padding: 6px 8px; border-radius: 6px; }
  .menu button:hover:not(:disabled) { background: var(--bg-selected); }
  .meta { margin-top: 2px; font-size: 12px; }
  .src { font-family: ui-monospace, monospace; font-size: 11px; }
  .seg { display: inline-flex; background: var(--bg-hover); border-radius: 7px; padding: 2px; margin-right: 6px; }
  .seg button { border: none; background: transparent; padding: 3px 10px; border-radius: 5px; color: var(--fg-muted); }
  .seg button.on { background: var(--bg); color: var(--fg); box-shadow: 0 1px 2px rgba(0,0,0,.12); }
  .seg.small button { font-size: 12px; padding: 2px 8px; }
  .speakers { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: 12px; }
  .spacer { flex: 1; }
  .chip-wrap { position: relative; }
  .chip { display: inline-flex; align-items: center; gap: 6px; border-radius: 14px; padding: 3px 10px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; }
  .sim { font-size: 10px; background: var(--bg-selected); color: var(--accent); border-radius: 6px; padding: 0 5px; }
  .picker {
    position: absolute; top: 32px; left: 0; width: 300px; background: var(--bg); border: 1px solid var(--border);
    border-radius: 10px; box-shadow: var(--shadow); padding: 8px; z-index: 20;
  }
  .options { display: flex; flex-direction: column; margin-top: 6px; max-height: 240px; overflow-y: auto; }
  .options button { text-align: left; border: none; background: transparent; padding: 5px 8px; border-radius: 6px; display: flex; gap: 5px; align-items: center; }
  .options button:hover { background: var(--bg-selected); }
  .hint { font-size: 11px; margin: 6px 4px 0; }
  .banner { display: flex; align-items: center; gap: 8px; padding: 6px 28px; font-size: 12px; background: var(--bg-card); border-bottom: 1px solid var(--border); color: var(--fg-muted); }
  .banner.warn { color: #b7791f; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .spinner { width: 11px; height: 11px; border: 2px solid var(--border); border-top-color: var(--accent); border-radius: 50%; animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .state { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; color: var(--fg-muted); padding: 40px; text-align: center; }
  .state p { margin: 0; max-width: 520px; user-select: text; }
  .stage { font-size: 15px; color: var(--fg); font-weight: 500; }
  .bar { width: 320px; height: 6px; border-radius: 3px; background: var(--border); overflow: hidden; }
  .bar span { display: block; height: 100%; background: var(--accent); transition: width 0.3s; }
  .list { flex: 1; overflow-y: auto; padding: 16px 28px 40px; }
  .utt { border-radius: 8px; }
  .who { margin: 14px 0 2px 52px; font-weight: 600; font-size: 12.5px; }
  .line { display: flex; gap: 8px; align-items: baseline; padding: 3px 6px 3px 0; border-radius: 8px; }
  .utt.active .line { background: var(--bg-selected); }
  .ts { border: none; background: transparent; color: var(--fg-faint); font-size: 11px; font-variant-numeric: tabular-nums; width: 46px; text-align: right; padding: 0 4px; flex-shrink: 0; }
  .ts:hover { color: var(--accent); }
  .text { flex: 1; font-size: 14.5px; line-height: 1.6; outline: none; user-select: text; -webkit-user-select: text; cursor: text; border-radius: 4px; }
  .text:focus { background: var(--bg-card); box-shadow: 0 0 0 4px var(--bg-card); }
  .protocol { flex: 1; overflow-y: auto; padding: 20px 28px 40px; max-width: 860px; user-select: text; }
  .protocol section { margin-bottom: 18px; }
  .protocol p, .protocol li { font-size: 14px; line-height: 1.55; margin: 0; }
  .protocol ul { margin: 0; padding-left: 20px; }
  table { border-collapse: collapse; width: 100%; font-size: 13px; }
  th, td { border: 1px solid var(--border); padding: 6px 8px; text-align: left; vertical-align: top; }
  th { background: var(--bg-card); font-weight: 600; }
  .proto-actions { display: flex; gap: 8px; margin-top: 10px; }
  .player { display: flex; align-items: center; gap: 6px; padding: 8px 16px; border-top: 1px solid var(--border); background: var(--bg-sidebar); }
  .play { width: 34px; height: 34px; border-radius: 50%; display: grid; place-items: center; padding: 0; background: var(--accent); color: white; border: none; }
  .time { font-size: 11.5px; font-variant-numeric: tabular-nums; min-width: 42px; text-align: center; }
  .scrub { flex: 1; accent-color: var(--accent); }
  .rate { width: auto; padding: 2px 4px; font-size: 12px; }
  .follow { display: flex; align-items: center; gap: 4px; font-size: 11.5px; }
</style>
