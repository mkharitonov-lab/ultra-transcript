<script lang="ts">
  import { fmtSize, t } from "../i18n.svelte";
  import { models } from "../models.svelte";
  import { app } from "../state.svelte";
  import Group from "../ui/Group.svelte";
  import Progress from "../ui/Progress.svelte";
  import Row from "../ui/Row.svelte";
  import Toggle from "../ui/Toggle.svelte";

  const s = $derived(app.settings!);
  const save = () => app.saveSettings();
  const denoiser = $derived(app.info?.models.find((m) => m.kind === "denoise"));
  const loading = $derived(denoiser ? models.progress(denoiser.name) : undefined);

  /** Шумоподавлению нужна модель: включили — скачиваем. */
  function denoise(on: boolean) {
    save();
    if (on && denoiser && !denoiser.installed && loading === undefined) models.download([denoiser.name]);
  }
</script>

<Group hint={t("set.audio.hint")}>
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
