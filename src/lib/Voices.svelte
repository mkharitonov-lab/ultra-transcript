<script lang="ts">
  import { onMount } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import { api, fmtTime, mediaFilter, showError, type Person, type VoiceSample } from "./api";
  import { fmtDate, t } from "./i18n.svelte";
  import { confirm as ask } from "./ui/dialog.svelte";

  /** Образцы голоса человека: послушать, убрать неудачный, загрузить из файла. `onchange` — сколько их теперь. */
  let { person, onchange }: { person: Person; onchange: (count: number) => void } = $props();
  let samples = $state<VoiceSample[] | null>(null);
  let playing = $state<number | null>(null);
  let busy = $state(false);
  let player = $state<HTMLAudioElement>();
  let loaded = ""; // какой файл сейчас в плеере
  let seekTo: number | null = null; // начало фрагмента — перемотать, когда файл откроется
  let stopAt = 0;

  async function load() {
    samples = await api.voices(person.id!).catch((e) => (showError(e), [] as VoiceSample[]));
    onchange(samples.length);
  }
  onMount(load);

  function toggle(s: VoiceSample) {
    if (!player) return;
    if (playing === s.id) return player.pause();
    stopAt = s.end;
    if (loaded !== s.audio) {
      loaded = s.audio;
      seekTo = s.start;
      player.src = convertFileSrc(s.audio);
    } else {
      player.currentTime = s.start;
    }
    playing = s.id;
    player.play().catch((e) => {
      // Переключились на другой образец, пока этот открывался, — не ошибка.
      if (e?.name === "AbortError") return;
      playing = null;
      showError(e);
    });
  }

  async function remove(s: VoiceSample) {
    if (!(await ask({ title: t("voices.deleteTitle"), text: t("voices.deleteText"), confirm: t("voices.delete"), danger: true }))) return;
    if (playing === s.id) player?.pause();
    await api.deleteVoice(s.id).catch(showError);
    await load();
  }

  async function upload() {
    const path = await open({ filters: [mediaFilter()] });
    if (typeof path !== "string") return;
    busy = true;
    try {
      await api.addVoice(person.id!, path);
      await load();
    } catch (e) {
      showError(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="voices">
  <audio
    bind:this={player}
    onloadedmetadata={() => {
      if (player && seekTo !== null) player.currentTime = seekTo;
      seekTo = null;
    }}
    ontimeupdate={() => {
      if (player && playing !== null && player.currentTime >= stopAt) player.pause();
    }}
    onpause={() => {
      if (player?.paused) playing = null;
    }}
    onended={() => (playing = null)}
  ></audio>
  {#if samples === null}
    <p class="faint">{t("common.loading")}</p>
  {:else}
    {#each samples as s (s.id)}
      <div class="sample">
        <button class="play" onclick={() => toggle(s)} disabled={!s.audio} aria-label={t(playing === s.id ? "player.pause" : "voices.listen")}
          title={s.audio ? t(playing === s.id ? "player.pause" : "voices.listen") : t("voices.noAudio")}>
          <Icon name={playing === s.id ? "pause" : "play"} size={10} />
        </button>
        <div class="about">
          <div class="head">
            <span class="title">{s.title || t("voices.deletedRecording")}</span>
            <span class="faint">{t(s.uploaded ? "voices.fromFile" : "voices.fromRecording")} · {fmtDate(s.date, false)}{s.audio ? ` · ${fmtTime(s.end - s.start)}` : ""}</span>
          </div>
          {#if s.text}
            <div class="quote muted">«{s.text}»</div>
          {:else if !s.audio}
            <div class="quote faint">{t("voices.noAudioHint")}</div>
          {/if}
        </div>
        <button class="ghost" onclick={() => remove(s)} title={t("voices.delete")} aria-label={t("voices.delete")}><Icon name="x" size={12} /></button>
      </div>
    {:else}
      <p class="muted empty">{t("voices.empty")}</p>
    {/each}
    <div class="add">
      <button class="with-icon" onclick={upload} disabled={busy}>
        {#if busy}<span class="spinner"></span> {t("status.processing")}…{:else}<Icon name="upload" size={12} /> {t("voices.upload")}{/if}
      </button>
      <span class="faint">{t("voices.uploadHint")}</span>
    </div>
  {/if}
</div>

<style>
  .voices { margin: 2px 0 8px; padding: 6px 10px 10px; border-radius: var(--r-md); background: var(--bg-card); border: 1px solid var(--border); }
  .sample { display: flex; align-items: center; gap: 10px; padding: 6px 0; border-bottom: 1px solid var(--border); }
  .play {
    width: 24px; height: 24px; flex-shrink: 0; border-radius: 50%; padding: 0; display: grid; place-items: center;
    background: var(--accent); color: var(--accent-fg); border: none;
  }
  .play:disabled { background: var(--bg-hover); color: var(--fg-faint); opacity: 1; }
  .about { flex: 1; min-width: 0; }
  .head { display: flex; gap: 8px; align-items: baseline; min-width: 0; }
  .title { font-weight: 550; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .head .faint { font-size: 11.5px; white-space: nowrap; }
  .quote { font-size: var(--fs-sm); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .empty { margin: 6px 0; }
  .add { display: flex; align-items: center; gap: 10px; padding-top: 10px; }
  .add .faint { font-size: var(--fs-sm); }
</style>
