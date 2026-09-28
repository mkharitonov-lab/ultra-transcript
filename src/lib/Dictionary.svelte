<script lang="ts">
  import { onMount, tick } from "svelte";
  import Page from "./Page.svelte";
  import Icon from "./Icon.svelte";
  import { api, confirmDialog, showError, type Term } from "./api";

  let terms = $state<Term[]>([]);
  let q = $state("");
  const shown = $derived(terms.filter((t) => !q || [t.term, t.aliases, t.definition].join(" ").toLowerCase().includes(q.toLowerCase())));

  onMount(async () => (terms = await api.terms()));

  async function save(t: Term) {
    if (!t.term.trim()) return;
    t.id = await api.saveTerm($state.snapshot(t)).catch((e) => (showError(e), t.id));
  }
  async function add() {
    q = "";
    terms.unshift({ id: null, term: "", aliases: "", definition: "" });
    await tick();
    (document.querySelector(".grid tbody input") as HTMLInputElement)?.focus();
  }
  async function remove(i: number) {
    const t = shown[i];
    if (t.id && !(await confirmDialog(`Удалить «${t.term}»?`))) return;
    if (t.id) await api.deleteTerm(t.id);
    terms = terms.filter((x) => x !== t);
  }
</script>

<Page title="Словарь" subtitle="Термины, аббревиатуры и названия. Пишутся в расшифровке строго так, как здесь. «Как слышится» — варианты, в которых модель ошибается: они заменяются автоматически.">
  {#snippet actions()}
    <input type="search" placeholder="Поиск" bind:value={q} style="width:200px" />
    <button class="primary" onclick={add}><Icon name="plus" size={12} /> Термин</button>
  {/snippet}
  {#if terms.length}
    <table class="grid">
      <thead><tr><th style="width:22%">Термин</th><th style="width:28%">Как слышится</th><th>Определение</th><th></th></tr></thead>
      <tbody>
        {#each shown as t, i (t)}
          <tr>
            <td><input type="text" bind:value={t.term} onchange={() => save(t)} placeholder="Минпромторг" /></td>
            <td><input type="text" bind:value={t.aliases} onchange={() => save(t)} placeholder="минпром торг, мин промторг" /></td>
            <td><input type="text" bind:value={t.definition} onchange={() => save(t)} placeholder="Министерство промышленности и торговли РФ" /></td>
            <td class="tools"><button class="ghost" onclick={() => remove(i)} title="Удалить"><Icon name="trash" /></button></td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="empty-state">Словарь пуст. Добавьте термины вручную — или они появятся в «Предложениях» после расшифровок.</p>
  {/if}
</Page>
