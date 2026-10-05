<script lang="ts">
  import { api } from "../api";
  import { t } from "../i18n.svelte";
  import { app } from "../state.svelte";
  import Group from "../ui/Group.svelte";
  import Row from "../ui/Row.svelte";
</script>

<p class="muted intro">{t("set.dev.hint")}</p>

<Group>
  <Row label="Nemotron 3" hint={app.info?.nemotron_runtime ? t("set.dev.nemotronReady") : t("set.dev.nemotronMissing")}>
    {#if !app.info?.nemotron_runtime}<code>src-tauri/scripts/build-nemo-speech.sh</code>{/if}
  </Row>
</Group>

<Group>
  <Row label={t("set.models.data")} hint={app.info?.data_dir}>
    <button onclick={() => app.info && api.reveal(app.info.data_dir, true)}>{t("common.open")}</button>
  </Row>
</Group>

<style>
  .intro { margin: 0 2px 18px; max-width: 620px; }
  code { font-size: var(--fs-sm); }
</style>
