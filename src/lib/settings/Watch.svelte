<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import Icon from "../Icon.svelte";
  import { api, showError, type Rule } from "../api";
  import { t } from "../i18n.svelte";
  import { app } from "../state.svelte";
  import { confirm } from "../ui/dialog.svelte";
  import Row from "../ui/Row.svelte";
  import Toggle from "../ui/Toggle.svelte";

  type Dir = "input_dir" | "audio_dir" | "transcript_dir" | "protocol_dir";

  let rules = $state<Rule[]>([]);
  onMount(async () => (rules = await api.rules().catch((e) => (showError(e), []))));

  const outputs: { key: Dir; label: "set.watch.transcripts" | "set.watch.protocols" | "set.watch.audio" }[] = [
    { key: "transcript_dir", label: "set.watch.transcripts" },
    { key: "protocol_dir", label: "set.watch.protocols" },
    { key: "audio_dir", label: "set.watch.audio" },
  ];

  async function save(r: Rule) {
    if (!r.input_dir) return;
    r.id = await api.saveRule($state.snapshot(r)).catch((e) => (showError(e), r.id));
  }

  async function pick(r: Rule, key: Dir) {
    const d = await open({ directory: true, defaultPath: r[key] || undefined });
    if (typeof d !== "string") return;
    r[key] = d;
    await save(r);
  }

  /** Новое правило появляется, когда выбрана папка: пустых карточек не бывает. */
  async function add() {
    const d = await open({ directory: true });
    if (typeof d !== "string") return;
    if (rules.some((r) => r.input_dir === d)) return;
    const r: Rule = { id: null, input_dir: d, audio_dir: "", transcript_dir: "", protocol_dir: "", protocol: app.settings?.llm_enabled ?? false, docx: false, enabled: true };
    rules.push(r);
    await save(rules[rules.length - 1]);
  }

  async function remove(r: Rule) {
    if (!(await confirm({ title: t("set.watch.deleteTitle"), text: t("set.watch.deleteText"), confirm: t("set.watch.stop"), danger: true }))) return;
    if (r.id) await api.deleteRule(r.id).catch(showError);
    rules = rules.filter((x) => x !== r);
  }

  /** Путь короче: домашняя папка — «~». */
  const short = (path: string) => path.replace(/^\/Users\/[^/]+/, "~").replace(/^[A-Z]:\\Users\\[^\\]+/i, "~");
</script>

<p class="muted intro">{t("set.watch.hint")}</p>

{#each rules as r (r)}
  <section class="rule" class:off={!r.enabled}>
    <header>
      <button class="source" onclick={() => pick(r, "input_dir")} title={r.input_dir}>
        <Icon name="inbox" size={15} /> <span>{short(r.input_dir)}</span>
      </button>
      <Toggle bind:checked={r.enabled} onchange={() => save(r)} label={t("set.watch.enabled")} />
      <button class="ghost icon" onclick={() => remove(r)} title={t("set.watch.stop")} aria-label={t("set.watch.stop")}><Icon name="trash" /></button>
    </header>
    <div class="rows">
      {#each outputs as o (o.key)}
        <Row label={t(o.label)}>
          {#if r[o.key]}
            <button class="ghost icon" title={t("set.watch.noCopy")} aria-label={t("set.watch.noCopy")} onclick={() => { r[o.key] = ""; save(r); }}><Icon name="x" size={12} /></button>
          {/if}
          <button class="path" class:unset={!r[o.key]} onclick={() => pick(r, o.key)} title={r[o.key]}>
            <Icon name="folder" size={13} /> <span>{r[o.key] ? short(r[o.key]) : t("set.watch.choose")}</span>
          </button>
        </Row>
      {/each}
      <Row label={t("set.watch.protocol")} hint={app.settings?.llm_enabled ? "" : t("set.watch.protocolNeedsLlm")}>
        <Toggle bind:checked={r.protocol} onchange={() => save(r)} label={t("set.watch.protocol")} />
      </Row>
      <Row label={t("set.watch.docx")} hint={t("set.watch.docxHint")}>
        <Toggle bind:checked={r.docx} onchange={() => save(r)} label={t("set.watch.docx")} />
      </Row>
    </div>
  </section>
{:else}
  <div class="empty">
    <Icon name="inbox" size={30} />
    <p>{t("set.watch.empty")}</p>
  </div>
{/each}

<button class="primary add" onclick={add}><Icon name="plus" size={13} /> {t("set.watch.add")}</button>

<style>
  .intro { margin: 0 2px 16px; max-width: 620px; }
  .rule { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--bg-card); margin-bottom: 14px; overflow: hidden; }
  .rule.off .rows { opacity: 0.55; }
  header { display: flex; align-items: center; gap: 10px; padding: 10px 10px 10px 8px; border-bottom: 1px solid var(--border); }
  .source {
    flex: 1; min-width: 0; display: flex; align-items: center; gap: 8px; border: none; background: transparent;
    font-weight: 600; text-align: left; padding: 4px 8px;
  }
  .source span, .path span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rows > :global(* + *) { border-top: 1px solid var(--border); }
  .path { display: flex; align-items: center; gap: 6px; max-width: 300px; }
  .path.unset { color: var(--fg-muted); }
  .icon { display: grid; padding: 5px; }
  .empty { display: flex; flex-direction: column; align-items: center; gap: 8px; padding: 28px 0 22px; color: var(--fg-faint); text-align: center; }
  .empty p { margin: 0; max-width: 420px; color: var(--fg-muted); }
  .add { display: inline-flex; align-items: center; gap: 6px; padding: 6px 14px; }
</style>
