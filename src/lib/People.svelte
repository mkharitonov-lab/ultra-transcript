<script lang="ts">
  import { tick, untrack } from "svelte";
  import Page from "./Page.svelte";
  import Icon from "./Icon.svelte";
  import ImportExport from "./ImportExport.svelte";
  import Voices from "./Voices.svelte";
  import { api, showError, type Person } from "./api";
  import { t, tn } from "./i18n.svelte";
  import { app } from "./state.svelte";
  import { confirm as ask } from "./ui/dialog.svelte";
  import { openMenu, type MenuItem } from "./ui/menu.svelte";

  let people = $state<Person[]>([]);
  let q = $state("");
  /** Чьи образцы голоса раскрыты. */
  let voicesOf = $state<number | null>(null);
  const shown = $derived(people.filter((p) => !q || [p.name, p.aliases, p.role, p.org].join(" ").toLowerCase().includes(q.toLowerCase())));
  const review = $derived(people.filter((p) => p.pending));

  // Счётчик находок в меню изменился — подгружаем новые.
  $effect(() => {
    app.pending.people;
    untrack(sync);
  });

  // Новые записи (находки из фоновой расшифровки) — сверху; строки, которые сейчас правят, не трогаем.
  async function sync() {
    const fresh = await api.people().catch((e) => (showError(e), []));
    people.unshift(...fresh.filter((f) => !people.some((p) => p.id === f.id || (p.id === null && p.name === f.name))));
  }
  async function reload() {
    people = await api.people().catch((e) => (showError(e), people));
    app.refresh();
  }

  async function save(p: Person) {
    if (!p.name.trim()) return;
    p.id = await api.savePerson($state.snapshot(p)).catch((e) => (showError(e), p.id));
  }
  async function add() {
    q = "";
    people.unshift({ id: null, name: "", aliases: "", role: "", org: "", voiceprints: 0, pending: false });
    await tick();
    document.querySelector<HTMLInputElement>(".grid tbody input")?.focus();
  }
  async function accept(p: Person) {
    if (!p.name.trim()) return;
    p.pending = false;
    await save(p);
    app.refresh();
  }
  async function remove(p: Person) {
    if (p.id && !p.pending) {
      const ok = await ask({
        title: t("people.deleteTitle", { name: p.name }),
        text: p.voiceprints ? t("people.deleteText") : "",
        confirm: t("common.delete"),
        danger: true,
      });
      if (!ok) return;
    }
    if (p.id) {
      try {
        await api.deletePerson(p.id);
      } catch (e) {
        return showError(e);
      }
    }
    people = people.filter((x) => x !== p);
    if (p.pending) app.refresh();
  }
  async function acceptAll() {
    for (const p of review) {
      p.pending = false;
      await save(p);
    }
    app.refresh();
  }
  async function removeAll() {
    const items = review;
    if (!(await ask({ title: tn("people.rejectAllTitle", items.length), text: t("review.rejectAllText"), confirm: t("common.delete"), danger: true }))) return;
    try {
      for (const p of items) await api.deletePerson(p.id!);
    } catch (e) {
      showError(e);
    }
    await reload();
  }
  function voicesChanged(p: Person, count: number) {
    p.voiceprints = count;
    // Загруженный голос подтверждает находку LLM (это делает и ядро).
    if (count && p.pending) {
      p.pending = false;
      app.refresh();
    }
  }
  const toggleVoices = (p: Person) => (voicesOf = voicesOf === p.id ? null : p.id);

  const menu = (p: Person): MenuItem[] => [
    ...(p.pending ? [{ label: t("review.accept"), icon: "check", action: () => accept(p) }] : []),
    ...(p.id ? [{ label: t(p.voiceprints ? "people.voiceListen" : "people.voiceAdd"), icon: "mic", action: () => toggleVoices(p) }] : []),
    { separator: true },
    { label: t(p.pending ? "review.reject" : "common.delete"), icon: "trash", danger: true, action: () => remove(p) },
  ];
</script>

<Page title={t("nav.people")} subtitle={t("people.subtitle")}>
  {#snippet actions()}
    <input type="search" placeholder={t("common.search")} bind:value={q} style="width:180px" />
    <ImportExport kind="people" onimport={reload} />
    <button class="primary with-icon" onclick={add}><Icon name="plus" size={12} /> {t("people.add")}</button>
  {/snippet}
  {#if review.length}
    <div class="review">
      <span class="text"><b>{tn("people.review", review.length)}</b> {t("people.reviewText")}</span>
      <button onclick={removeAll}>{t("review.rejectAll")}</button>
      <button class="primary with-icon" onclick={acceptAll}><Icon name="check" size={12} /> {t("review.acceptAll")}</button>
    </div>
  {/if}
  {#if people.length}
    <table class="grid">
      <thead>
        <tr>
          <th style="width:24%">{t("people.name")}</th><th style="width:18%">{t("people.aliases")}</th>
          <th style="width:20%">{t("people.role")}</th><th>{t("people.org")}</th><th style="width:132px">{t("people.voice")}</th><th class="tools"></th>
        </tr>
      </thead>
      <tbody>
        {#each shown as p (p)}
          <tr class:pending={p.pending} oncontextmenu={(e) => { if (!(e.target instanceof HTMLInputElement)) openMenu(e, menu(p)); }}>
            <td><input type="text" bind:value={p.name} onchange={() => save(p)} placeholder={p.id ? "" : t("people.nameExample")} /></td>
            <td><input type="text" bind:value={p.aliases} onchange={() => save(p)} placeholder={p.id ? "" : t("people.aliasesExample")} /></td>
            <td><input type="text" bind:value={p.role} onchange={() => save(p)} placeholder={p.id ? "" : t("people.roleExample")} /></td>
            <td><input type="text" bind:value={p.org} onchange={() => save(p)} /></td>
            <td class="voice-cell">
              {#if p.id}
                <button class="ghost voice" class:has={p.voiceprints > 0} class:open={voicesOf === p.id} aria-expanded={voicesOf === p.id}
                  onclick={() => toggleVoices(p)} title={t(p.voiceprints ? "people.voiceListen" : "people.voiceAdd")}>
                  {#if p.voiceprints}<Icon name="mic" size={12} /> {tn("people.samples", p.voiceprints)}{:else}<Icon name="plus" size={11} /> {t("people.voiceNone")}{/if}
                  <span class="twist"><Icon name="chevron" size={10} /></span>
                </button>
              {:else}<span class="faint">—</span>{/if}
            </td>
            <td class="tools">
              {#if p.pending}
                <button class="ghost confirm" onclick={() => accept(p)} title={t("review.accept")} aria-label={t("review.accept")}><Icon name="check" /></button>
                <button class="ghost" onclick={() => remove(p)} title={t("review.rejectHint")} aria-label={t("review.reject")}><Icon name="x" /></button>
              {:else}
                <button class="ghost" onclick={() => remove(p)} title={t("common.delete")} aria-label={t("common.delete")}><Icon name="trash" /></button>
              {/if}
            </td>
          </tr>
          {#if p.id && voicesOf === p.id}
            <tr><td colspan="6"><Voices person={p} onchange={(n) => voicesChanged(p, n)} /></td></tr>
          {/if}
        {:else}
          <tr><td colspan="6" class="empty-state">{t("side.nothingFound")}</td></tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <div class="empty-state">
      <Icon name="people" size={30} />
      <p>{t("people.empty")}</p>
      <p class="faint">{t("people.emptyLlm")}</p>
    </div>
  {/if}
</Page>

<style>
  .voice-cell { white-space: nowrap; }
  .voice { display: inline-flex; align-items: center; gap: 5px; font-size: var(--fs-sm); color: var(--fg-muted); padding: 5px 8px; }
  .voice.has { color: var(--ok); }
  .twist { display: grid; transition: transform 0.15s; }
  .voice.open .twist { transform: rotate(180deg); }
</style>
