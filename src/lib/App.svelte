<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, mediaFilter, showError } from "./api";
  import Dictionary from "./Dictionary.svelte";
  import Icon from "./Icon.svelte";
  import People from "./People.svelte";
  import Recording from "./Recording.svelte";
  import Sidebar from "./Sidebar.svelte";
  import Welcome from "./Welcome.svelte";
  import { t } from "./i18n.svelte";
  import { live } from "./live.svelte";
  import { models } from "./models.svelte";
  import { newFolder, startRecording } from "./recordings";
  import Settings from "./settings/Settings.svelte";
  import { app } from "./state.svelte";

  let dragging = $state(false);

  async function addFiles() {
    const picked = await open({ multiple: true, filters: [mediaFilter()] });
    if (picked) await importPaths(Array.isArray(picked) ? picked : [picked]);
  }

  async function importPaths(paths: string[]) {
    const ids = await api.importFiles(paths).catch((e) => (showError(e), [] as string[]));
    await app.refresh();
    if (ids.length) app.open(ids[0]);
  }

  // На macOS эти сочетания есть и в строке меню — до окна они тогда не доходят.
  function shortcuts(e: KeyboardEvent) {
    if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
    const key = e.key.toLowerCase();
    if (key === ",") app.go("settings");
    else if (key === "o") addFiles();
    else if (key === "r") startRecording();
    else if (key === "n" && e.shiftKey) newFolder();
    else return;
    e.preventDefault();
  }

  onMount(() => {
    app.init().then(() => live.restore(), showError);
    const subs = [
      api.onJob((e) => {
        if (live.onJob(e)) app.refresh();
      }),
      api.onLive((e) => live.onLive(e)),
      api.onRecord((e) => live.onRecord(e)),
      api.onModels((e) => models.onEvent(e)),
      api.onMenu((action) => {
        if (action === "add") addFiles();
        else if (action === "record") startRecording();
        else if (action === "folder") newFolder();
        else app.go("settings", action === "about" ? "about" : undefined);
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
</script>

<svelte:window onkeydown={shortcuts} />

<div class="app">
  <Sidebar onadd={addFiles} />
  <main>
    <div class="titlebar" data-tauri-drag-region></div>
    {#if app.info && app.settings}
      {#if app.view === "recording" && app.current}
        {#key app.current.id}<Recording recording={app.current} />{/key}
      {:else if app.view === "dictionary"}<Dictionary />
      {:else if app.view === "people"}<People />
      {:else if app.view === "settings"}<Settings />
      {:else}<Welcome onadd={addFiles} />{/if}
    {/if}
  </main>

  {#if dragging}
    <div class="drop-overlay"><div><Icon name="download" size={36} /><p>{t("drop.release")}</p></div></div>
  {/if}
</div>

<style>
  .app { display: grid; grid-template-columns: 264px minmax(0, 1fr); height: 100vh; }
  main { position: relative; min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  .titlebar { position: absolute; top: 0; left: 0; right: 0; height: 34px; z-index: 5; }
  .drop-overlay {
    position: fixed; inset: 0; background: color-mix(in srgb, var(--accent) 12%, var(--bg) 60%);
    border: 3px dashed var(--accent); display: grid; place-items: center; z-index: 500; pointer-events: none;
    color: var(--accent); font-size: 16px; font-weight: 600; text-align: center;
  }
</style>
