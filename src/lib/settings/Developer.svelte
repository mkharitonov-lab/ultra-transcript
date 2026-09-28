<script lang="ts">
  import { api } from "../api";
  import { t } from "../i18n.svelte";
  import { app } from "../state.svelte";
  import Group from "../ui/Group.svelte";
  import Row from "../ui/Row.svelte";

  const s = $derived(app.settings!);
  const save = () => app.saveSettings();
  const kbps = [16, 24, 32, 48];
</script>

<p class="muted intro">{t("set.dev.hint")}</p>

<Group title={t("set.dev.speakers")}>
  <Row label={t("set.dev.cluster")} hint={t("set.dev.clusterHint")}>
    <input type="range" min="0.2" max="0.8" step="0.05" bind:value={s.cluster_threshold} onchange={save} disabled={s.diar_model !== "pyannote3"} />
    <span class="num">{s.cluster_threshold.toFixed(2)}</span>
  </Row>
  <Row label={t("set.dev.voice")} hint={t("set.dev.voiceHint")}>
    <input type="range" min="0.35" max="0.85" step="0.05" bind:value={s.voice_threshold} onchange={save} />
    <span class="num">{s.voice_threshold.toFixed(2)}</span>
  </Row>
  <Row label="Nemotron 3" hint={app.info?.nemotron_runtime ? t("set.dev.nemotronReady") : t("set.dev.nemotronMissing")}>
    {#if !app.info?.nemotron_runtime}<code>src-tauri/scripts/build-nemo-speech.sh</code>{/if}
  </Row>
</Group>

<Group title={t("set.dev.files")}>
  <Row label={t("set.dev.bitrate")} hint={t("set.dev.bitrateHint")}>
    <select bind:value={s.archive_kbps} onchange={save}>
      {#each kbps as k (k)}<option value={k}>{t("set.dev.kbps", { k, mb: Math.round(k * 0.45) })}</option>{/each}
    </select>
  </Row>
  <Row label={t("set.models.data")} hint={app.info?.data_dir}>
    <button onclick={() => app.info && api.reveal(app.info.data_dir, true)}>{t("common.open")}</button>
  </Row>
</Group>

<style>
  .intro { margin: 0 2px 18px; max-width: 620px; }
  input[type="range"] { width: 180px; accent-color: var(--accent); }
  .num { width: 34px; text-align: right; font-variant-numeric: tabular-nums; color: var(--fg-muted); }
  code { font-size: var(--fs-sm); }
</style>
