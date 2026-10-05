<script lang="ts">
  import type { DiarModel } from "../api";
  import { t } from "../i18n.svelte";
  import { app } from "../state.svelte";
  import Group from "../ui/Group.svelte";
  import Row from "../ui/Row.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import ModelChoice from "./ModelChoice.svelte";

  const s = $derived(app.settings!);
  const save = () => app.saveSettings();
  const model = (id: DiarModel) => app.info?.models.find((m) => m.diar === id);

  // Выключатель запоминает, какой способ был выбран, и возвращает его при включении.
  const LAST = "diar.last";
  const split = $derived(s.diar_model !== "off");
  function toggle(on: boolean) {
    if (on) s.diar_model = (localStorage.getItem(LAST) as DiarModel | null) ?? "pyannote3";
    else {
      localStorage.setItem(LAST, s.diar_model);
      s.diar_model = "off";
    }
    save();
  }

  const engines = $derived([
    { value: "pyannote3" as DiarModel, title: t("set.diar.pyannote"), model: model("pyannote3") },
    { value: "community1" as DiarModel, title: t("set.diar.community"), model: model("community1") },
    // Nemotron 3 требует отдельно собранной библиотеки — без неё вариант не показываем.
    ...(app.info?.nemotron_runtime
      ? [{ value: "nemotron3" as DiarModel, title: t("set.diar.nemotron"), model: model("nemotron3") }]
      : []),
  ]);
</script>

<Group>
  <Row label={t("set.diar.split")} hint={t("set.diar.splitHint")}>
    <Toggle checked={split} onchange={toggle} label={t("set.diar.split")} />
  </Row>
</Group>

{#if split}
  <Group title={t("set.diar.engine")} hint={t("set.diar.engineHint")} bare>
    <ModelChoice bind:value={s.diar_model} onchange={save} options={engines} />
  </Group>
{/if}

<!-- Строгость разделения и узнавания: раньше — только в режиме разработчика. -->
<Group title={t("set.diar.strictness")}>
  {#if s.diar_model === "pyannote3"}
    <Row label={t("set.dev.cluster")} hint={t("set.dev.clusterHint")}>
      <input type="range" min="0.2" max="0.8" step="0.05" bind:value={s.cluster_threshold} onchange={save} />
      <span class="num">{s.cluster_threshold.toFixed(2)}</span>
    </Row>
  {/if}
  <Row label={t("set.dev.voice")} hint={t("set.dev.voiceHint")}>
    <input type="range" min="0.35" max="0.85" step="0.05" bind:value={s.voice_threshold} onchange={save} />
    <span class="num">{s.voice_threshold.toFixed(2)}</span>
  </Row>
</Group>

<Group title={t("set.voices.title")}>
  <Row label={t("set.voices.how")} hint={t("set.voices.howHint")}>
    <button onclick={() => app.go("people")}>{t("set.voices.open")}</button>
  </Row>
</Group>

<style>
  input[type="range"] { width: 180px; accent-color: var(--accent); }
  .num { width: 34px; text-align: right; font-variant-numeric: tabular-nums; color: var(--fg-muted); }
</style>
