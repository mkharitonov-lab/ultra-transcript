<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api } from "../api";
  import { t } from "../i18n.svelte";
  import { app } from "../state.svelte";
  import Group from "../ui/Group.svelte";
  import Row from "../ui/Row.svelte";

  const s = $derived(app.settings!);
  const save = () => app.saveSettings();
  const fileName = (path: string) => path.split(/[\\/]/).pop() ?? "";

  async function pick(key: "transcript_template" | "protocol_template") {
    const f = await open({ filters: [{ name: "Word", extensions: ["docx"] }], defaultPath: app.info?.templates_dir });
    if (typeof f !== "string") return;
    s[key] = f;
    save();
  }
</script>

{#snippet template(key: "transcript_template" | "protocol_template", label: string)}
  <Row {label}>
    {#if s[key]}<button class="ghost" onclick={() => { s[key] = ""; save(); }}>{t("common.reset")}</button>{/if}
    <button class="file" onclick={() => pick(key)} title={s[key]}>{fileName(s[key]) || t("set.docs.standard")}</button>
  </Row>
{/snippet}

<Group title={t("set.docs.formats")}>
  <Row label="Markdown (.md)" hint={t("set.docs.mdHint")} />
  <Row label="Word (.docx)" hint={t("set.docs.docxHint")} />
</Group>

<Group title={t("set.docs.templates")} hint={t("set.docs.templatesHint")}>
  {@render template("transcript_template", t("set.docs.transcript"))}
  {@render template("protocol_template", t("set.docs.protocol"))}
  <Row label={t("set.docs.folder")} hint={t("set.docs.folderHint")}>
    <button onclick={() => app.info && api.reveal(app.info.templates_dir, true)}>{t("common.open")}</button>
  </Row>
</Group>

<style>
  .file { max-width: 260px; overflow: hidden; text-overflow: ellipsis; }
</style>
