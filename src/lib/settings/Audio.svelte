<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../api";
  import { fmtSize, t } from "../i18n.svelte";
  import { models } from "../models.svelte";
  import { app } from "../state.svelte";
  import Group from "../ui/Group.svelte";
  import Progress from "../ui/Progress.svelte";
  import Row from "../ui/Row.svelte";
  import Toggle from "../ui/Toggle.svelte";

  const s = $derived(app.settings!);
  const kbps = [16, 24, 32, 48];
  const save = () => app.saveSettings();
  const denoiser = $derived(app.info?.models.find((m) => m.kind === "denoise"));
  const loading = $derived(denoiser ? models.progress(denoiser.name) : undefined);

  /** Микрофоны, которые есть сейчас; выбранный, но отключённый, тоже остаётся в списке. */
  let devices = $state<string[]>([]);
  onMount(() => {
    api.inputDevices().then((d) => (devices = d), () => {});
  });
  const choices = $derived(s.input_device && !devices.includes(s.input_device) ? [...devices, s.input_device] : devices);
  function pickDevice(e: Event) {
    s.input_device = (e.currentTarget as HTMLSelectElement).value;
    save();
  }

  /** Шумоподавлению нужна модель: включили — скачиваем. */
  function denoise(on: boolean) {
    save();
    if (on && denoiser && !denoiser.installed && loading === undefined) models.download([denoiser.name]);
  }
</script>

<Group title={t("set.audio.record")}>
  <Row label={t("set.audio.mic")} hint={t("set.audio.micHint")}>
    <select value={s.input_device} onchange={pickDevice}>
      <option value="">{t("set.audio.micDefault")}{devices[0] ? ` — ${devices[0]}` : ""}</option>
      {#each choices as d (d)}<option value={d}>{d}</option>{/each}
    </select>
  </Row>
</Group>

<Group title={t("set.audio.prepare")} hint={t("set.audio.hint")}>
  <Row label={t("set.audio.denoise")} hint={t("set.audio.denoiseHint")}>
    {#if loading !== undefined}
      <Progress value={loading} width="96px" />
    {:else if s.denoise && denoiser && !denoiser.installed}
      <button onclick={() => models.download([denoiser.name])}>{t("common.download")} · {fmtSize(denoiser.size_mb)}</button>
    {/if}
    <Toggle bind:checked={s.denoise} onchange={denoise} label={t("set.audio.denoise")} />
  </Row>
  <Row label={t("set.audio.level")} hint={t("set.audio.levelHint")}>
    <Toggle bind:checked={s.level_volume} onchange={save} label={t("set.audio.level")} />
  </Row>
</Group>

<Group title={t("set.audio.archive")}>
  <Row label={t("set.dev.bitrate")} hint={t("set.dev.bitrateHint")}>
    <select bind:value={s.archive_kbps} onchange={save}>
      {#each kbps as k (k)}<option value={k}>{t("set.dev.kbps", { k, mb: Math.round(k * 0.45) })}</option>{/each}
    </select>
  </Row>
</Group>
