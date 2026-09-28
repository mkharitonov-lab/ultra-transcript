<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, confirmDialog, fmtTime, showError, type AppInfo, type Recording } from "$lib/api";
  import Icon from "$lib/Icon.svelte";
  import Home from "$lib/Home.svelte";
  import RecordingView from "$lib/RecordingView.svelte";
  import Dictionary from "$lib/Dictionary.svelte";
  import People from "$lib/People.svelte";
  import Suggestions from "$lib/Suggestions.svelte";
  import Folders from "$lib/Folders.svelte";
  import SettingsView from "$lib/Settings.svelte";

  type View = "home" | "recording" | "dictionary" | "people" | "suggestions" | "folders" | "settings";

  let info = $state<AppInfo | null>(null);
  let recordings = $state<Recording[]>([]);
  let progress = $state<Record<string, { stage: string; progress: number }>>({});
  let view = $state<View>("home");
  let selected = $state<string | null>(null);
  let suggestionCount = $state(0);
  let dragging = $state(false);
  let filter = $state("");

  const shown = $derived(
    recordings.filter((r) => !filter || r.title.toLowerCase().includes(filter.toLowerCase())),
  );
  const current = $derived(recordings.find((r) => r.id === selected) ?? null);

  async function refresh() {
    recordings = await api.recordings();
    suggestionCount = (await api.suggestions()).length;
  }

  async function addFiles() {
    const picked = await open({
      multiple: true,
      filters: [{ name: "Аудио и видео", extensions: ["mp3", "m4a", "wav", "aac", "ogg", "opus", "flac", "wma", "amr", "aiff", "caf", "mp4", "mov", "mkv", "avi", "webm", "wmv", "m4v", "mpeg", "mpg"] }],
    });
    if (picked) await importPaths(Array.isArray(picked) ? picked : [picked]);
  }

  async function importPaths(paths: string[]) {
    const ids = await api.importFiles(paths).catch((e) => (showError(e), [] as string[]));
    await refresh();
    if (ids.length) select(ids[0]);
  }

  function select(id: string) {
    selected = id;
    view = "recording";
  }

  function go(v: View) {
    view = v;
    selected = null;
  }

  async function remove(id: string) {
    if (!(await confirmDialog("Удалить запись, расшифровку и архив аудио?"))) return;
    await api.deleteRecording(id);
    if (selected === id) go("home");
    await refresh();
  }

  onMount(() => {
    api.appInfo().then((i) => (info = i));
    refresh();
    const subs = [
      api.onJob((e) => {
        if (e.status === "processing") progress[e.recording_id] = { stage: e.stage, progress: e.progress };
        else delete progress[e.recording_id];
        const r = recordings.find((r) => r.id === e.recording_id);
        if (!r || r.status !== e.status) refresh();
      }),
      getCurrentWebview().onDragDropEvent((e) => {
        if (e.payload.type === "over" || e.payload.type === "enter") dragging = true;
        else if (e.payload.type === "leave") dragging = false;
        else if (e.payload.type === "drop") {
          dragging = false;
          importPaths(e.payload.paths);
        }
      }),
    ];
    return () => subs.forEach((s) => s.then((un) => un()));
  });

  const nav: [View, string, string][] = [
    ["dictionary", "Словарь", "book"],
    ["people", "Кто есть кто", "people"],
    ["suggestions", "Предложения", "sparkle"],
    ["folders", "Папки", "folder"],
    ["settings", "Настройки", "gear"],
  ];
</script>

<div class="app" class:dragging>
  <aside>
    <div class="titlebar" data-tauri-drag-region></div>
    <div class="side-top">
      <button class="primary add" onclick={addFiles}><Icon name="plus" /> Добавить файлы</button>
      <input type="search" placeholder="Поиск" bind:value={filter} />
    </div>
    <div class="section-label">Библиотека</div>
    <nav class="library">
      {#each shown as r (r.id)}
        <button
          class="item"
          class:active={selected === r.id}
          onclick={() => select(r.id)}
          oncontextmenu={(e) => { e.preventDefault(); remove(r.id); }}
        >
          <span class="status {r.status}">
            {#if r.status === "processing"}<span class="spinner"></span>
            {:else if r.status === "queued"}<Icon name="clock" size={13} />
            {:else if r.status === "error"}<Icon name="alert" size={13} />
            {:else}<Icon name="wave" size={13} />{/if}
          </span>
          <span class="item-body">
            <span class="item-title">{r.title}</span>
            <span class="item-meta">
              {#if r.status === "processing" && progress[r.id]}
                {progress[r.id].stage || "Обработка"}{progress[r.id].progress > 0 ? ` · ${Math.round(progress[r.id].progress * 100)}%` : ""}
              {:else if r.status === "queued"}В очереди
              {:else if r.status === "error"}Ошибка
              {:else}{r.created_at} · {fmtTime(r.duration)}{r.rule_id ? " · из папки" : ""}{/if}
            </span>
          </span>
        </button>
      {:else}
        <div class="empty faint">Пока пусто</div>
      {/each}
    </nav>
    <nav class="bottom">
      {#each nav as [v, label, icon]}
        <button class="item nav" class:active={view === v} onclick={() => go(v)}>
          <Icon name={icon} size={15} /> <span>{label}</span>
          {#if v === "suggestions" && suggestionCount}<span class="badge">{suggestionCount}</span>{/if}
        </button>
      {/each}
    </nav>
  </aside>

  <main>
    <div class="titlebar main-bar" data-tauri-drag-region></div>
    {#if info}
      {#if view === "recording" && current}
        {#key current.id}
          <RecordingView recording={current} {info} progress={progress[current.id]} onchange={refresh} onremove={() => remove(current.id)} />
        {/key}
      {:else if view === "dictionary"}<Dictionary />
      {:else if view === "people"}<People />
      {:else if view === "suggestions"}<Suggestions onchange={refresh} />
      {:else if view === "folders"}<Folders />
      {:else if view === "settings"}<SettingsView {info} onmodels={() => api.appInfo().then((i) => (info = i))} />
      {:else}<Home {info} onadd={addFiles} onmodels={() => api.appInfo().then((i) => (info = i))} />
      {/if}
    {/if}
  </main>

  {#if dragging}
    <div class="drop-overlay"><div><Icon name="download" size={36} /><p>Отпустите, чтобы расшифровать</p></div></div>
  {/if}
</div>

<style>
  .app { display: grid; grid-template-columns: 260px 1fr; height: 100vh; }
  aside {
    background: var(--bg-sidebar); border-right: 1px solid var(--border);
    display: flex; flex-direction: column; min-height: 0;
  }
  .titlebar { height: 38px; flex-shrink: 0; }
  .main-bar { position: absolute; top: 0; left: 260px; right: 0; z-index: 5; }
  .side-top { padding: 0 12px 10px; display: flex; flex-direction: column; gap: 8px; }
  .add { display: flex; align-items: center; justify-content: center; gap: 6px; padding: 6px; }
  .section-label { padding: 6px 18px 4px; font-size: 11px; font-weight: 600; color: var(--fg-faint); text-transform: uppercase; letter-spacing: 0.04em; }
  .library { flex: 1; overflow-y: auto; padding: 0 8px; min-height: 0; }
  .bottom { padding: 8px; border-top: 1px solid var(--border); }
  .item {
    display: flex; align-items: center; gap: 9px; width: 100%; text-align: left;
    border: none; background: transparent; padding: 6px 8px; border-radius: 7px; margin-bottom: 1px;
  }
  .item:hover { background: var(--bg-hover); }
  .item.active { background: var(--bg-selected); }
  .item.nav { padding: 5px 8px; color: var(--fg); }
  .item.nav span { flex: 1; }
  .badge { flex: 0 !important; background: var(--accent); color: white; border-radius: 9px; padding: 0 6px; font-size: 11px; }
  .item-body { display: flex; flex-direction: column; min-width: 0; flex: 1; }
  .item-title { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-weight: 500; }
  .item-meta { font-size: 11px; color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .status { width: 24px; height: 24px; border-radius: 6px; display: grid; place-items: center; flex-shrink: 0; background: var(--bg-card); color: var(--fg-muted); border: 1px solid var(--border); }
  .status.done { color: var(--accent); }
  .status.error { color: var(--danger); }
  .spinner { width: 12px; height: 12px; border: 2px solid var(--border); border-top-color: var(--accent); border-radius: 50%; animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .empty { padding: 12px 10px; }
  main { position: relative; min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  .drop-overlay {
    position: fixed; inset: 0; background: color-mix(in srgb, var(--accent) 12%, transparent);
    border: 3px dashed var(--accent); display: grid; place-items: center; z-index: 50; pointer-events: none;
    color: var(--accent); font-size: 16px; font-weight: 600; text-align: center;
  }
</style>
