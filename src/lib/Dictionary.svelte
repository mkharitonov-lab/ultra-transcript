<script lang="ts">
  import { tick, untrack } from "svelte";
  import Page from "./Page.svelte";
  import Icon from "./Icon.svelte";
  import ImportExport from "./ImportExport.svelte";
  import { api, showError, type Term } from "./api";
  import { t, tn } from "./i18n.svelte";
  import { app } from "./state.svelte";
  import { confirm as ask } from "./ui/dialog.svelte";
  import { openMenu, type MenuItem } from "./ui/menu.svelte";

  let terms = $state<Term[]>([]);
  let q = $state("");
  const shown = $derived(terms.filter((x) => !q || [x.term, x.aliases, x.definition].join(" ").toLowerCase().includes(q.toLowerCase())));
  const review = $derived(terms.filter((x) => x.pending));

  // Счётчик находок в меню изменился — подгружаем новые.
  $effect(() => {
    app.pending.terms;
    untrack(sync);
  });

  // Новые записи (находки из фоновой расшифровки) — сверху; строки, которые сейчас правят, не трогаем.
  async function sync() {
    const fresh = await api.terms().catch((e) => (showError(e), []));
    terms.unshift(...fresh.filter((f) => !terms.some((x) => x.id === f.id || (x.id === null && x.term === f.term))));
  }
  async function reload() {
    terms = await api.terms().catch((e) => (showError(e), terms));
    app.refresh();
  }

  async function save(x: Term) {
    if (!x.term.trim()) return;
    x.id = await api.saveTerm($state.snapshot(x)).catch((e) => (showError(e), x.id));
  }
  async function add() {
    q = "";
    terms.unshift({ id: null, term: "", aliases: "", definition: "", pending: false });
    await tick();
    document.querySelector<HTMLInputElement>(".grid tbody input")?.focus();
  }
  async function accept(x: Term) {
    if (!x.term.trim()) return;
    x.pending = false;
    await save(x);
    app.refresh();
  }
  async function remove(x: Term) {
    if (x.id && !x.pending && !(await ask({ title: t("dict.deleteTitle", { name: x.term }), confirm: t("common.delete"), danger: true }))) return;
    if (x.id) {
      try {
        await api.deleteTerm(x.id);
      } catch (e) {
        return showError(e);
      }
    }
    terms = terms.filter((y) => y !== x);
    if (x.pending) app.refresh();
  }
  async function acceptAll() {
    for (const x of review) {
      x.pending = false;
      await save(x);
    }
    app.refresh();
  }
  async function removeAll() {
    const items = review;
    if (!(await ask({ title: tn("dict.rejectAllTitle", items.length), text: t("review.rejectAllText"), confirm: t("common.delete"), danger: true }))) return;
    try {
      for (const x of items) await api.deleteTerm(x.id!);
    } catch (e) {
      showError(e);
    }
    await reload();
  }

  const menu = (x: Term): MenuItem[] => [
    ...(x.pending ? [{ label: t("review.accept"), icon: "check", action: () => accept(x) }] : []),
    { label: t(x.pending ? "review.reject" : "common.delete"), icon: "trash", danger: true, action: () => remove(x) },
  ];
</script>

<Page title={t("nav.dictionary")} subtitle={t("dict.subtitle")}>
  {#snippet actions()}
    <input type="search" placeholder={t("common.search")} bind:value={q} style="width:180px" />
    <ImportExport kind="terms" onimport={reload} />
    <button class="primary with-icon" onclick={add}><Icon name="plus" size={12} /> {t("dict.add")}</button>
  {/snippet}
  {#if review.length}
    <div class="review">
      <span class="text"><b>{tn("dict.review", review.length)}</b> {t("dict.reviewText")}</span>
      <button onclick={removeAll}>{t("review.rejectAll")}</button>
      <button class="primary with-icon" onclick={acceptAll}><Icon name="check" size={12} /> {t("review.acceptAll")}</button>
    </div>
  {/if}
  {#if terms.length}
    <table class="grid">
      <thead><tr><th style="width:22%">{t("dict.term")}</th><th style="width:28%">{t("dict.aliases")}</th><th>{t("dict.definition")}</th><th class="tools"></th></tr></thead>
      <tbody>
        {#each shown as x (x)}
          <tr class:pending={x.pending} oncontextmenu={(e) => { if (!(e.target instanceof HTMLInputElement)) openMenu(e, menu(x)); }}>
            <td><input type="text" bind:value={x.term} onchange={() => save(x)} placeholder={x.id ? "" : t("dict.termExample")} /></td>
            <td><input type="text" bind:value={x.aliases} onchange={() => save(x)} placeholder={x.id ? "" : t("dict.aliasesExample")} /></td>
            <td><input type="text" bind:value={x.definition} onchange={() => save(x)} placeholder={x.id ? "" : t("dict.definitionExample")} /></td>
            <td class="tools">
              {#if x.pending}
                <button class="ghost confirm" onclick={() => accept(x)} title={t("review.accept")} aria-label={t("review.accept")}><Icon name="check" /></button>
                <button class="ghost" onclick={() => remove(x)} title={t("review.rejectHint")} aria-label={t("review.reject")}><Icon name="x" /></button>
              {:else}
                <button class="ghost" onclick={() => remove(x)} title={t("common.delete")} aria-label={t("common.delete")}><Icon name="trash" /></button>
              {/if}
            </td>
          </tr>
        {:else}
          <tr><td colspan="4" class="empty-state">{t("side.nothingFound")}</td></tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <div class="empty-state">
      <Icon name="book" size={30} />
      <p>{t("dict.empty")}</p>
      <p class="faint">{t("dict.emptyLlm")}</p>
    </div>
  {/if}
</Page>
