<script lang="ts">
  import { onMount, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import LivePane from "./LivePane.svelte";
  import ProtocolView from "./ProtocolView.svelte";
  import { api, audioUrl, copyText, fmtTime, showError, speakerColor, type Person, type Recording, type Speaker, type Transcript, type Utterance } from "./api";
  import { fmtDate, t, tn } from "./i18n.svelte";
  import { live, parsePartialJson } from "./live.svelte";
  import { copyDoc, exportFile, makeProtocol, recordingMenu } from "./recordings";
  import { app } from "./state.svelte";
  import { confirm } from "./ui/dialog.svelte";
  import { openMenu, type MenuItem } from "./ui/menu.svelte";
  import Progress from "./ui/Progress.svelte";
  import Segmented from "./ui/Segmented.svelte";
  import { textItems } from "./ui/textmenu";
  import { toast } from "./ui/toast.svelte";

  let { recording }: { recording: Recording } = $props();

  let tr = $state<Transcript | null>(null);
  let tab = $state<"transcript" | "protocol">("transcript");
  let people = $state<Person[]>([]);
  let picking = $state<string | null>(null);
  let query = $state("");
  let audio = $state<HTMLAudioElement>();
  let time = $state(0);
  let paused = $state(true);
  let rate = $state(1);
  /** Есть ли у записи обработанный звук (узнаём, попробовав его открыть) и играет ли он. */
  let hasClean = $state(true);
  let clean = $state(true);
  let follow = $state(true);
  let list = $state<HTMLElement>();

  const job = $derived(live.of(recording.id));
  /** Запись в работе: расшифровывается, ждёт очереди или по ней пишется протокол. */
  const busy = $derived(recording.status === "processing" || recording.status === "queued" || !!job);
  /** Идёт новая расшифровка, а её черновика ещё нет: показываем текст по мере распознавания. */
  const streaming = $derived(recording.status === "processing" && job?.job === "transcribe" && job.drafts === 0);
  const writing = $derived(job?.job === "protocol");
  const draftProtocol = $derived(writing && job?.protocol ? (parsePartialJson(job.protocol) as Record<string, unknown> | null) : null);
  const active = $derived(tr ? tr.utterances.findLastIndex((u) => u.start <= time + 0.05) : -1);
  const filtered = $derived(people.filter((p) => !query || p.name.toLowerCase().includes(query.toLowerCase())));
  const kind = $derived(tab === "protocol" && tr?.protocol ? "protocol" : "transcript");

  async function load() {
    tr = await api.transcript(recording.id).catch(() => null);
  }

  // Черновик обновился или задача завершилась — перечитываем; текст, который сейчас правят, не трогаем.
  let seen = { working: false, drafts: 0 };
  $effect(() => {
    const now = { working: !!job || recording.status === "processing", drafts: job?.drafts ?? 0 };
    const finished = seen.working && !now.working;
    if ((finished || now.drafts > seen.drafts) && !document.activeElement?.closest?.(".text")) load();
    // Новая расшифровка могла сохранить обработанный звук.
    if (finished) hasClean = true;
    seen = now;
  });
  $effect(() => {
    if (writing) tab = "protocol";
  });

  onMount(() => {
    load();
    api.people().then((p) => (people = p), showError);
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement;
      if (e.code === "Space" && !el.isContentEditable && !["INPUT", "TEXTAREA", "BUTTON", "SELECT"].includes(el.tagName)) {
        e.preventDefault();
        toggle();
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

  const toggle = () => audio && (paused ? audio.play() : audio.pause());
  /** Обработанный звук ↔ исходная запись — с того же места. */
  function switchSound() {
    const [at, playing] = [time, !paused];
    clean = !clean;
    audio?.addEventListener("loadedmetadata", () => {
      if (!audio) return;
      audio.currentTime = at;
      if (playing) audio.play();
    }, { once: true });
  }
  function seek(sec: number) {
    if (!audio) return;
    audio.currentTime = sec;
    audio.play();
  }

  async function save() {
    if (!tr) return;
    await api.saveTranscript($state.snapshot(tr) as Transcript).catch(showError);
    app.refresh();
  }

  function editText(u: Utterance, el: HTMLElement) {
    const value = el.innerText.trim();
    if (shown(u) === value) return;
    u.clean = value;
    save();
  }

  async function renameTitle(e: Event) {
    const field = e.target as HTMLInputElement;
    const v = field.value.trim();
    if (!v) field.value = recording.title;
    else if (v !== recording.title) {
      if (tr) tr.title = v;
      await api.renameRecording(recording.id, v).catch(showError);
      app.refresh();
    }
  }

  async function assign(speaker: string, personId: number | null, name = "") {
    picking = null;
    query = "";
    try {
      tr = await api.assignSpeaker(recording.id, speaker, personId, name);
      people = await api.people();
      app.refresh();
      if (personId !== null || name.trim()) toast(t("rec.voiceSaved"), "ok");
    } catch (e) {
      showError(e);
    }
  }

  async function openPicker(id: string) {
    picking = picking === id ? null : id;
    query = "";
    await tick();
    document.querySelector<HTMLInputElement>(".picker input")?.focus();
  }

  function pickerKey(e: KeyboardEvent, s: Speaker) {
    if (e.key === "Escape") picking = null;
    if (e.key === "Enter" && query.trim()) {
      const exact = people.find((p) => p.name.toLowerCase() === query.trim().toLowerCase());
      assign(s.id, exact?.id ?? filtered[0]?.id ?? null, exact || filtered.length ? "" : query);
    }
  }

  function setSpeaker(u: Utterance, id: string) {
    u.speaker = id;
    save();
  }

  // ---------- меню ----------

  const speakerMenu = (s: Speaker): MenuItem[] => [
    { label: t(s.person_id ? "rec.changePerson" : "rec.whoIsIt"), icon: "people", action: () => openPicker(s.id) },
    ...(s.person_id ? [{ label: t("rec.resetPerson"), icon: "x", action: () => assign(s.id, null, "") }] : []),
  ];

  /** Текст реплики после редактуры; без LLM — как распознан. */
  const shown = (u: Utterance) => u.clean || u.text;

  const utteranceMenu = (u: Utterance): MenuItem[] => [
    { label: t("rec.playFrom"), icon: "play", hint: fmtTime(u.start), action: () => seek(u.start) },
    { label: t("rec.copyUtterance"), icon: "copy", action: () => copyText(shown(u)).catch(showError) },
    ...(tr && tr.speakers.length > 1 && !busy
      ? [{
          label: t("rec.speaker"), icon: "people",
          items: tr.speakers.map((s): MenuItem => ({ label: s.name, checked: s.id === u.speaker, action: () => setSpeaker(u, s.id) })),
        }]
      : []),
  ];

  const exportMenu = (): MenuItem[] => [
    { heading: t(kind === "protocol" ? "export.protocol" : "export.transcript") },
    { label: t("export.copy"), icon: "copy", action: () => copyDoc(recording, kind) },
    { separator: true },
    { label: t("export.saveMd"), icon: "doc", action: () => exportFile(recording, kind, "md", "save") },
    { label: t("export.saveDocx"), icon: "doc", action: () => exportFile(recording, kind, "docx", "save") },
    { separator: true },
    { label: t("export.openDocx"), icon: "reveal", action: () => exportFile(recording, kind, "docx", "open") },
  ];

  /** Предупреждение целиком: в полосе над текстом помещается только первая строка. */
  const showWarning = () => confirm({ title: t("rec.warning"), text: recording.error, confirm: t("common.ok"), cancel: null });
</script>

<div class="view">
  <header data-tauri-drag-region>
    <div class="head-row" data-tauri-drag-region>
      <input class="title" value={recording.title} onchange={renameTitle} spellcheck="false" aria-label={t("rec.title")}
        onkeydown={(e) => { if (e.key === "Enter" || e.key === "Escape") e.currentTarget.blur(); }} />
      <div class="actions">
        {#if tr}
          <Segmented bind:value={tab} options={[
            { value: "transcript", label: t("rec.transcript") },
            { value: "protocol", label: t("rec.protocol") },
          ]} />
          <button class="with-icon" onclick={(e) => openMenu(e, exportMenu())} disabled={streaming}><Icon name="upload" /> {t("rec.export")}</button>
        {/if}
        <button class="ghost icon" onclick={(e) => openMenu(e, recordingMenu([recording]))} title={t("rec.actions")} aria-label={t("rec.actions")}><Icon name="more" size={16} /></button>
      </div>
    </div>
    <div class="meta muted">
      {fmtDate(recording.created_at)} · {fmtTime(tr?.duration ?? recording.duration)}
      {#if tr && !streaming} · {tn("rec.speakers", tr.speakers.length)}{/if}
      {#if tr && app.developer}{#if tr.asr_model} · {tr.asr_model}{/if}{#if tr.diar_model} · {tr.diar_model}{/if}{/if}
    </div>

    {#if tr && tab === "transcript" && !streaming}
      <div class="speakers">
        {#each tr.speakers as s (s.id)}
          <div class="chip-wrap">
            <button class="chip" onclick={() => openPicker(s.id)} oncontextmenu={(e) => openMenu(e, speakerMenu(s))} disabled={busy}>
              <span class="dot" style="background:{speakerColor(s.id)}"></span>
              {s.name}
              {#if s.similarity}<span class="sim" title={t("rec.byVoice")}>{Math.round(s.similarity * 100)}%</span>{/if}
              {#if !s.person_id}<span class="faint">· {t("rec.whoIsIt")}</span>{/if}
            </button>
            {#if picking === s.id}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="backdrop" onmousedown={() => (picking = null)}></div>
              <div class="picker">
                <input type="text" placeholder={t("rec.pickerPlaceholder")} bind:value={query} onkeydown={(e) => pickerKey(e, s)} spellcheck="false" />
                <div class="options">
                  {#each filtered.slice(0, 8) as p (p.id)}
                    <button onclick={() => assign(s.id, p.id)}>
                      <span class="grow">{p.name}</span>
                      {#if p.role}<span class="faint">{p.role}</span>{/if}
                      {#if p.voiceprints}<span class="voice" title={t("rec.hasVoice")}><Icon name="mic" size={11} /></span>{/if}
                    </button>
                  {/each}
                  {#if query.trim() && !people.some((p) => p.name.toLowerCase() === query.trim().toLowerCase())}
                    <button onclick={() => assign(s.id, null, query)}><Icon name="plus" size={12} /> {t("rec.addPerson", { name: query.trim() })}</button>
                  {/if}
                  {#if s.person_id}<button class="danger" onclick={() => assign(s.id, null, "")}>{t("rec.resetPerson")}</button>{/if}
                </div>
                <p class="faint hint">{t("rec.pickerHint")}</p>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </header>

  {#if recording.status === "queued" && tr}
    <div class="banner"><Icon name="clock" /> {t("rec.queued")}</div>
  {:else if job && tr && !streaming}
    <div class="banner">
      <span class="spinner"></span>
      <span>{job?.title || t("status.processing")}{job && job.progress > 0 ? ` · ${Math.round(job.progress * 100)}%` : ""}</span>
      <Progress value={job?.progress ?? 0} width="140px" />
    </div>
  {:else if recording.error && tr}
    <button class="banner warn" onclick={showWarning}><Icon name="alert" /> <span>{recording.error.split("\n")[0]}</span></button>
  {/if}

  {#if streaming || (!tr && recording.status === "processing")}
    <LivePane {job} />
  {:else if !tr}
    <div class="state">
      {#if recording.status === "error"}
        <span class="big danger"><Icon name="alert" size={30} /></span>
        <h2>{t("rec.failed")}</h2>
        <p class="muted">{recording.error}</p>
        <button class="with-icon" onclick={() => api.retry(recording.id).then(() => app.refresh(), showError)}><Icon name="refresh" /> {t("rec.retry")}</button>
      {:else}
        <span class="big"><Icon name="clock" size={30} /></span>
        <h2>{t("rec.queued")}</h2>
        <p class="muted">{t("rec.background")}</p>
      {/if}
    </div>
  {:else if tab === "transcript"}
    <div class="list" bind:this={list}>
      {#each tr.utterances as u, i (u.id)}
        {@const prev = tr.utterances[i - 1]}
        {@const speaker = tr.speakers.find((s) => s.id === u.speaker)}
        <div class="utt" class:active={i === active && !paused} data-i={i}>
          {#if !prev || prev.speaker !== u.speaker}
            <div class="who">
              <button class="name" style="color:{speakerColor(u.speaker)}" disabled={busy || !speaker}
                onclick={() => speaker && openPicker(speaker.id)}
                oncontextmenu={(e) => speaker && openMenu(e, speakerMenu(speaker))}>{speaker?.name ?? u.speaker}</button>
            </div>
          {/if}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="line" oncontextmenu={(e) => openMenu(e, [...textItems(e), { separator: true }, ...utteranceMenu(u)])}>
            <button class="ts" onclick={() => seek(u.start)} title={t("rec.playFrom")}>{fmtTime(u.start)}</button>
            {#key job?.drafts ?? 0}
              <div class="text" contenteditable={busy ? "false" : "plaintext-only"} spellcheck="false"
                onblur={(e) => editText(u, e.currentTarget)}
                onkeydown={(e) => { if (e.key === "Escape") e.currentTarget.blur(); }}>{shown(u)}</div>
            {/key}
          </div>
        </div>
      {:else}
        <div class="state"><h2>{t("rec.noSpeech")}</h2><p class="muted">{t("rec.noSpeechHint")}</p></div>
      {/each}
    </div>
  {:else if writing}
    <div class="protocol">
      {#if draftProtocol && Object.keys(draftProtocol).length}
        <ProtocolView data={draftProtocol} writing />
      {:else}
        <div class="state"><span class="spinner large"></span><h2>{job?.title}</h2><p class="muted">{t("rec.protocolWait")}</p></div>
      {/if}
    </div>
  {:else if tr.protocol}
    <div class="protocol">
      <ProtocolView data={tr.protocol} />
      <div class="proto-actions">
        <button class="ghost with-icon" onclick={() => makeProtocol(recording)} disabled={busy}><Icon name="refresh" /> {t("rec.protocolAgain")}</button>
      </div>
    </div>
  {:else}
    <div class="state">
      <span class="big"><Icon name="sparkle" size={30} /></span>
      <h2>{t("rec.protocolTitle")}</h2>
      {#if app.settings?.llm_enabled}
        <p class="muted">{t("rec.protocolHint")}</p>
        <button class="primary" onclick={() => makeProtocol(recording)} disabled={busy}>{t("rec.makeProtocol")}</button>
      {:else}
        <p class="muted">{t("rec.protocolNeedsLlm")}</p>
        <button onclick={() => app.go("settings", "llm")}>{t("rec.openLlm")}</button>
      {/if}
    </div>
  {/if}

  {#if tr && !streaming}
    <footer class="player">
      <audio bind:this={audio} src={audioUrl(app.info!.data_dir, recording.id, hasClean && clean)} onerror={() => (hasClean = false)}
        bind:currentTime={time} bind:paused bind:playbackRate={rate} preload="metadata"></audio>
      <button class="ghost icon" onclick={() => audio && (audio.currentTime -= 5)} title={t("player.back")} aria-label={t("player.back")}><Icon name="skip-back" size={15} /></button>
      <button class="play" onclick={toggle} aria-label={t(paused ? "player.play" : "player.pause")}><Icon name={paused ? "play" : "pause"} size={15} /></button>
      <button class="ghost icon" onclick={() => audio && (audio.currentTime += 5)} title={t("player.forward")} aria-label={t("player.forward")}><Icon name="skip-fwd" size={15} /></button>
      <span class="time">{fmtTime(time)}</span>
      <input class="scrub" type="range" min="0" max={tr.duration} step="0.1" bind:value={time} aria-label={t("player.position")}
        oninput={(e) => audio && (audio.currentTime = +e.currentTarget.value)} />
      <span class="time faint">{fmtTime(tr.duration)}</span>
      <button class="ghost rate" title={t("player.speed")}
        onclick={(e) => openMenu(e, [0.75, 1, 1.25, 1.5, 1.75, 2].map((r): MenuItem => ({ label: `${r}×`, checked: rate === r, action: () => (rate = r) })))}>{rate}×</button>
      {#if hasClean}
        <button class="ghost icon" class:on={clean} onclick={switchSound} aria-pressed={clean} title={t(clean ? "player.clean" : "player.original")} aria-label={t("player.cleanLabel")}><Icon name="sliders" size={15} /></button>
      {/if}
      <button class="ghost icon" class:on={follow} onclick={() => (follow = !follow)} aria-pressed={follow} title={t("player.follow")} aria-label={t("player.follow")}><Icon name="target" size={15} /></button>
    </footer>
  {/if}
</div>

<style>
  .view { display: flex; flex-direction: column; height: 100vh; min-height: 0; }
  header { padding: 34px 28px 10px; border-bottom: 1px solid var(--border); position: relative; z-index: 6; }
  .head-row { display: flex; align-items: center; gap: 12px; }
  .title {
    flex: 1; min-width: 0; font-size: var(--fs-xl); font-weight: 650; letter-spacing: -0.01em; text-overflow: ellipsis;
    border-color: transparent; background: transparent; padding: 2px 6px; margin-left: -7px;
  }
  .title:hover:not(:focus) { background: var(--bg-hover); }
  .actions { display: flex; align-items: center; gap: 6px; }
  .with-icon { display: inline-flex; align-items: center; gap: 6px; }
  .icon { display: grid; padding: 5px; }
  .icon.on { color: var(--accent); background: var(--bg-selected); }
  .meta { margin-top: 2px; font-size: var(--fs-sm); }

  .speakers { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: 12px; }
  .chip-wrap { position: relative; }
  .chip { display: inline-flex; align-items: center; gap: 6px; border-radius: 14px; padding: 3px 10px; }
  .chip:disabled { opacity: 1; }
  .dot { width: 8px; height: 8px; border-radius: 50%; }
  .sim { font-size: var(--fs-xs); background: var(--bg-selected); color: var(--accent); border-radius: var(--r-sm); padding: 0 5px; }
  .backdrop { position: fixed; inset: 0; z-index: 19; }
  .picker {
    position: absolute; top: 32px; left: 0; width: 300px; background: var(--bg-elevated); border: 1px solid var(--border);
    border-radius: var(--r-lg); box-shadow: var(--shadow-md); padding: 8px; z-index: 20;
  }
  .options { display: flex; flex-direction: column; margin-top: 6px; max-height: 240px; overflow-y: auto; }
  .options button { text-align: left; border: none; background: transparent; padding: 5px 8px; border-radius: var(--r-sm); display: flex; gap: 6px; align-items: center; }
  .options button:hover { background: var(--bg-selected); }
  .grow { flex: 1; overflow: hidden; text-overflow: ellipsis; }
  .voice { display: grid; color: var(--ok); }
  .hint { font-size: var(--fs-xs); margin: 6px 4px 0; }

  .banner {
    display: flex; align-items: center; gap: 8px; padding: 6px 28px; font-size: var(--fs-sm); text-align: left; width: 100%;
    background: var(--bg-card); border: none; border-bottom: 1px solid var(--border); border-radius: 0; color: var(--fg-muted);
  }
  .banner span { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .banner .spinner { flex: none; }
  .banner.warn { color: var(--warn); }

  .state { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; padding: 40px; text-align: center; }
  .state h2 { font-size: var(--fs-lg); margin: 4px 0 0; }
  .state p { margin: 0 0 8px; max-width: 480px; user-select: text; -webkit-user-select: text; overflow-wrap: anywhere; }
  .big { color: var(--fg-faint); display: grid; }
  .big.danger { color: var(--danger); }

  .list { flex: 1; overflow-y: auto; padding: 16px 28px 40px; display: flex; flex-direction: column; }
  .who { margin: 14px 0 2px 46px; }
  .name { border: none; background: transparent; padding: 0 6px; font-weight: 600; font-size: 12.5px; }
  .name:disabled { opacity: 1; }
  .line { display: flex; gap: 8px; align-items: baseline; padding: 3px 6px 3px 0; border-radius: var(--r-md); }
  .utt.active .line { background: var(--bg-selected); }
  .ts {
    border: none; background: transparent; color: var(--fg-faint); font-size: var(--fs-xs); font-variant-numeric: tabular-nums;
    width: 46px; text-align: right; padding: 0 4px; flex-shrink: 0;
  }
  .ts:hover { color: var(--accent); background: transparent; }
  .text {
    flex: 1; font-size: 14.5px; line-height: 1.6; outline: none; border-radius: 4px; max-width: 760px; white-space: pre-wrap;
    user-select: text; -webkit-user-select: text; cursor: text;
  }
  .text:focus { background: var(--bg-card); box-shadow: 0 0 0 4px var(--bg-card); }

  .protocol { flex: 1; overflow-y: auto; padding: 20px 28px 40px; display: flex; flex-direction: column; }
  .proto-actions { display: flex; gap: 8px; margin-top: 14px; }

  .player { display: flex; align-items: center; gap: 6px; padding: 8px 16px; border-top: 1px solid var(--border); background: var(--bg-sidebar); }
  .play { width: 32px; height: 32px; border-radius: 50%; display: grid; place-items: center; padding: 0; background: var(--accent); color: var(--accent-fg); border: none; }
  .play:hover { background: var(--accent); filter: brightness(1.08); }
  .time { font-size: 11.5px; font-variant-numeric: tabular-nums; min-width: 42px; text-align: center; }
  .scrub { flex: 1; accent-color: var(--accent); }
  .rate { font-size: var(--fs-sm); font-variant-numeric: tabular-nums; min-width: 44px; padding: 3px 6px; }
</style>
