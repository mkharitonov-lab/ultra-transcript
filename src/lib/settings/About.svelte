<script lang="ts">
  import icon from "../assets/icon.png";
  import { api, showError } from "../api";
  import { t } from "../i18n.svelte";
  import { app } from "../state.svelte";
  import Group from "../ui/Group.svelte";
  import Row from "../ui/Row.svelte";

  let doc = $state<{ title: string; text: string } | null>(null);
  async function show(kind: "license" | "notices", title: string) {
    doc = { title, text: await api.legal(kind).catch((e) => (showError(e), "")) };
  }
  const downloadable = $derived(app.info?.models ?? []);
</script>

<svelte:window onkeydowncapture={(e) => { if (doc && e.key === "Escape") { doc = null; e.stopPropagation(); } }} />

<header class="about">
  <img src={icon} alt="" width="72" height="72" />
  <div>
    <h1>{t("app.name")}</h1>
    <p class="muted">{t("about.tagline")}</p>
    <p class="version">{t("about.version", { version: app.info?.version ?? "" })}</p>
  </div>
</header>

<Group title={t("about.license")}>
  <Row label="PolyForm Noncommercial 1.0.0" hint={t("about.licenseHint")}>
    <button onclick={() => show("license", t("about.license"))}>{t("about.read")}</button>
  </Row>
</Group>

<Group title={t("about.privacy")}>
  <Row label={t("about.local")} hint={t("about.localHint")} />
</Group>

<Group title={t("about.models")} hint={t("about.modelsHint")}>
  {#each downloadable as m (m.name)}
    <div class="model">
      <span class="name">{m.title}</span>
      <span class="muted">{m.publisher}</span>
      <span class="muted license">{m.license}</span>
    </div>
  {/each}
</Group>

<Group title={t("about.components")}>
  <Row label={t("about.openSource")} hint={t("about.openSourceHint")}>
    <button onclick={() => show("notices", t("about.components"))}>{t("about.read")}</button>
  </Row>
</Group>

<p class="faint copyright">© 2026 Mikhail Kharitonov</p>

{#if doc}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="scrim" onmousedown={(e) => { if (e.target === e.currentTarget) doc = null; }}>
    <div class="sheet" role="dialog" aria-modal="true" aria-label={doc.title}>
      <header><h2>{doc.title}</h2><button onclick={() => (doc = null)}>{t("common.close")}</button></header>
      <pre>{doc.text}</pre>
    </div>
  </div>
{/if}

<style>
  .about { display: flex; align-items: center; gap: 18px; margin: 4px 2px 26px; }
  .about img { border-radius: 16px; }
  .about p { margin: 2px 0 0; }
  .version { font-size: var(--fs-sm); color: var(--fg-faint); font-variant-numeric: tabular-nums; user-select: text; -webkit-user-select: text; }
  .model { display: grid; grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr) minmax(0, 1.2fr); gap: 12px; padding: 8px 14px; font-size: var(--fs-sm); }
  .name { font-weight: 500; font-size: var(--fs-md); }
  .license { text-align: right; }
  .copyright { margin: 0 2px; font-size: var(--fs-sm); }
  .scrim { position: fixed; inset: 0; z-index: 600; display: grid; place-items: center; padding: 32px; background: var(--scrim); }
  .sheet {
    width: min(720px, 100%); max-height: 100%; display: flex; flex-direction: column; border-radius: var(--r-xl);
    background: var(--bg-elevated); border: 1px solid var(--border); box-shadow: var(--shadow-lg); overflow: hidden;
  }
  .sheet header { display: flex; align-items: center; justify-content: space-between; padding: 14px 16px 14px 20px; border-bottom: 1px solid var(--border); }
  .sheet h2 { margin: 0; font-size: var(--fs-lg); }
  pre {
    margin: 0; padding: 18px 20px 24px; overflow-y: auto; white-space: pre-wrap; overflow-wrap: anywhere;
    font: inherit; font-size: var(--fs-sm); line-height: 1.55; user-select: text; -webkit-user-select: text; cursor: text;
  }
</style>
