<script lang="ts">
  import type { LangSetting } from "../i18n.svelte";
  import { t } from "../i18n.svelte";
  import { app } from "../state.svelte";
  import Group from "../ui/Group.svelte";
  import Row from "../ui/Row.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Toggle from "../ui/Toggle.svelte";

  const s = $derived(app.settings!);
  const save = () => app.saveSettings();

  const languages: { value: LangSetting; label: string }[] = $derived([
    { value: "system", label: t("set.general.system") },
    { value: "ru", label: "Русский" },
    { value: "en", label: "English" },
  ]);
</script>

<Group title={t("set.general.look")}>
  <Row label={t("set.general.language")}>
    <select bind:value={s.language} onchange={save}>
      {#each languages as l (l.value)}<option value={l.value}>{l.label}</option>{/each}
    </select>
  </Row>
  <Row label={t("set.general.theme")}>
    <Segmented bind:value={s.theme} onchange={save} options={[
      { value: "system", label: t("set.general.themeSystem"), icon: "monitor" },
      { value: "light", label: t("set.general.themeLight"), icon: "sun" },
      { value: "dark", label: t("set.general.themeDark"), icon: "moon" },
    ]} />
  </Row>
</Group>

<Group title={t("set.general.background")}>
  <Row label={t("set.general.notify")} hint={t("set.general.notifyHint")}>
    <Toggle bind:checked={s.notifications} onchange={save} label={t("set.general.notify")} />
  </Row>
</Group>

<Group title={t("set.general.advanced")}>
  <Row label={t("set.general.developer")} hint={t("set.general.developerHint")}>
    <Toggle bind:checked={s.developer_mode} label={t("set.general.developer")}
      onchange={(on) => { save(); if (on) app.section = "developer"; }} />
  </Row>
</Group>
