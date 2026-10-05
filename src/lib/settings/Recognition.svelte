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
  const whisper = ["ru", "en", "uk", "be", "kk", "de", "fr", "es", "it", "pt", "pl", "tr", "zh", "ja", "ko"];
  /** Все языки Parakeet. */
  const parakeet = ["ru", "en", "uk", "de", "fr", "es", "it", "pt", "pl", "bg", "cs", "da", "el", "et", "fi", "hr", "hu", "lt",
    "lv", "mt", "nl", "ro", "sk", "sl", "sv"];
  /** Языки GLM-ASR-Nano: язык подсказывается ей в инструкции, без него она иногда переводит речь на английский. */
  const glm = ["ru", "en", "uk", "de", "fr", "es", "it", "pt", "nl", "ca", "zh", "ja", "ms", "id", "no", "lt", "sl"];
  const codes = $derived(s.asr_model === "parakeet" ? parakeet : s.asr_model === "glm-asr" ? glm : whisper);
  const languages = $derived.by(() => {
    const names = new Intl.DisplayNames([i18n.lang], { type: "language" });
    return codes.map((code) => {
      const name = names.of(code) ?? code;
      return { code, name: name[0].toUpperCase() + name.slice(1) };
    });
  });
  /** Язык, которого нет у выбранной модели, она определяет сама. */
  const language = $derived(codes.includes(s.speech_language) ? s.speech_language : "auto");
  function pickLanguage(e: Event) {
    s.speech_language = (e.currentTarget as HTMLSelectElement).value;
    save();
  }
</script>

<Group title={t("set.asr.model")} hint={t("set.asr.modelHint")} bare>
  <ModelChoice bind:value={s.asr_model} onchange={save} options={[
    { value: "gigaam", title: "GigaAM v3", model: model("gigaam") },
    { value: "whisper-turbo", title: "Whisper large-v3-turbo", model: model("whisper-turbo") },
    { value: "parakeet", title: "Parakeet TDT 0.6B v3", model: model("parakeet") },
    { value: "glm-asr", title: "GLM-ASR-Nano", model: model("glm-asr") },
  ]} />
</Group>

{#if s.asr_model !== "gigaam"}
  <Group>
    <Row label={t("set.asr.language")} hint={t("set.asr.languageHint")}>
      <select value={language} onchange={pickLanguage}>
        <option value="auto">{t("set.asr.auto")}</option>
        {#each languages as l (l.code)}<option value={l.code}>{l.name}</option>{/each}
      </select>
    </Row>
  </Group>
{/if}
