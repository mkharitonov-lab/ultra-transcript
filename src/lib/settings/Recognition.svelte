<script lang="ts">
  import type { AsrModel } from "../api";
  import { i18n, t } from "../i18n.svelte";
  import { app } from "../state.svelte";
  import Group from "../ui/Group.svelte";
  import Row from "../ui/Row.svelte";
  import ModelChoice from "./ModelChoice.svelte";

  const s = $derived(app.settings!);
  const save = () => app.saveSettings();
  const model = (id: AsrModel) => app.info?.models.find((m) => m.asr === id);

  /** Языки, которые чаще всего нужны; Whisper понимает и другие — их он определит сам. */
  const codes = ["ru", "en", "uk", "be", "kk", "de", "fr", "es", "it", "pt", "pl", "tr", "zh", "ja", "ko"];
  const languages = $derived.by(() => {
    const names = new Intl.DisplayNames([i18n.lang], { type: "language" });
    return codes.map((code) => {
      const name = names.of(code) ?? code;
      return { code, name: name[0].toUpperCase() + name.slice(1) };
    });
  });
</script>

<Group title={t("set.asr.model")} hint={t("set.asr.modelHint")} bare>
  <ModelChoice bind:value={s.asr_model} onchange={save} options={[
    { value: "gigaam", title: "GigaAM v3", about: t("set.asr.gigaam"), model: model("gigaam"), badge: t("set.asr.forRussian") },
    { value: "whisper-turbo", title: "Whisper large-v3-turbo", about: t("set.asr.whisper"), model: model("whisper-turbo") },
  ]} />
</Group>

{#if s.asr_model === "whisper-turbo"}
  <Group>
    <Row label={t("set.asr.language")} hint={t("set.asr.languageHint")}>
      <select bind:value={s.speech_language} onchange={save}>
        <option value="auto">{t("set.asr.auto")}</option>
        {#each languages as l (l.code)}<option value={l.code}>{l.name}</option>{/each}
      </select>
    </Row>
  </Group>
{/if}
