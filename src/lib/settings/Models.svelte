<script lang="ts">
  import Icon from "../Icon.svelte";
  import { api, type Model } from "../api";
  import { fmtSize, t, type Key } from "../i18n.svelte";
  import { models } from "../models.svelte";
  import { app } from "../state.svelte";
  import Group from "../ui/Group.svelte";
  import Progress from "../ui/Progress.svelte";
  import Row from "../ui/Row.svelte";

  const kinds: { kind: Model["kind"]; title: Key }[] = [
    { kind: "asr", title: "set.models.asr" },
    { kind: "diar", title: "set.models.diar" },
    { kind: "denoise", title: "set.models.denoise" },
    { kind: "llm", title: "set.models.llm" },
    { kind: "core", title: "set.models.core" },
  ];
  const s = $derived(app.settings!);
  // Nemotron 3 без своей библиотеки не работает — скачивать модель незачем.
  const all = $derived(
    (app.info?.models ?? []).filter((m) => m.name !== "nemotron3" || m.installed || app.info?.nemotron_runtime || app.developer),
  );
  const inUse = (m: Model) =>
    m.required ||
    (m.kind === "denoise" && s.denoise) ||
    (m.kind === "llm" && s.llm_enabled && s.llm_provider === "builtin" && s.llm_local_model === m.name);
  const used = $derived(all.filter((m) => m.installed).reduce((a, m) => a + m.size_mb, 0));
</script>

<p class="muted intro">{t("set.models.hint")}
  <button class="link" onclick={() => (app.section = "about")}>{t("set.models.licenses")}</button>
</p>

{#each kinds as k (k.kind)}
  {@const list = all.filter((m) => m.kind === k.kind)}
  {#if list.length}
    <Group title={t(k.title)}>
      {#each list as m (m.name)}
        {@const loading = models.progress(m.name)}
        <div class="model">
          <span class="dot" class:ok={m.installed} title={t(m.installed ? "common.installed" : "common.notInstalled")}></span>
          <div class="text">
            <div class="title">{m.title}{#if inUse(m)}<span class="tag">{t("set.models.inUse")}</span>{/if}</div>
            <div class="meta muted">{fmtSize(m.size_mb)} · {m.publisher} · {m.license}</div>
          </div>
          {#if loading !== undefined}
            <Progress value={loading} width="110px" />
          {:else if m.installed}
            <button class="ghost icon" onclick={() => models.remove(m)} disabled={m.required}
              title={m.required ? t("set.models.cantDelete") : t("set.models.delete")} aria-label={t("set.models.delete")}><Icon name="trash" /></button>
          {:else}
            <button onclick={() => models.download([m.name])}>{t("common.download")}</button>
          {/if}
        </div>
      {/each}
    </Group>
  {/if}
{/each}

<Group title={t("set.models.storage")}>
  <Row label={t("set.models.used")}><span class="muted">{fmtSize(used)}</span></Row>
  <Row label={t("set.models.data")} hint={t("set.models.dataHint")}>
    <button onclick={() => app.info && api.reveal(app.info.data_dir, true)}>{t("common.open")}</button>
  </Row>
</Group>

<style>
  .intro { margin: 0 2px 18px; max-width: 620px; }
  .model { display: flex; align-items: center; gap: 12px; padding: 9px 14px; min-height: 44px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--border-strong); flex-shrink: 0; }
  .dot.ok { background: var(--ok); }
  .text { flex: 1; min-width: 0; }
  .title { font-weight: 500; display: flex; align-items: center; gap: 8px; }
  .meta { font-size: var(--fs-sm); }
  .tag { font-size: var(--fs-xs); font-weight: 500; color: var(--accent); background: var(--bg-selected); border-radius: var(--r-sm); padding: 0 6px; }
  .icon { display: grid; padding: 5px; color: var(--fg-muted); }
</style>
