<script lang="ts">
  import { onMount } from "svelte";
  import Page from "./Page.svelte";
  import Icon from "./Icon.svelte";
  import { api, type Suggestion } from "./api";

  let { onchange }: { onchange: () => void } = $props();
  let items = $state<Suggestion[]>([]);

  onMount(async () => (items = await api.suggestions()));

  async function resolve(s: Suggestion, accept: boolean) {
    await api.resolveSuggestion(s.id, accept);
    items = items.filter((x) => x.id !== s.id);
    onchange();
  }
  async function all(accept: boolean) {
    for (const s of [...items]) await resolve(s, accept);
  }
</script>

<Page title="Предложения" subtitle="После каждой расшифровки LLM находит новые термины и имена. Примите нужные — они попадут в словарь и справочник «Кто есть кто».">
  {#snippet actions()}
    {#if items.length}
      <button onclick={() => all(false)}>Отклонить все</button>
      <button class="primary" onclick={() => all(true)}>Принять все</button>
    {/if}
  {/snippet}
  {#each items as s (s.id)}
    <div class="row">
      <span class="kind">{s.kind === "person" ? "Человек" : "Термин"}</span>
      <div class="body">
        <div class="value">{s.value}</div>
        {#if s.detail}<div class="muted">{s.detail}</div>{/if}
      </div>
      <button class="ghost" onclick={() => resolve(s, false)} title="Отклонить"><Icon name="x" /></button>
      <button onclick={() => resolve(s, true)}><Icon name="check" size={12} /> Добавить</button>
    </div>
  {:else}
    <p class="empty-state">Новых предложений нет. Они появятся после расшифровки, если включена LLM.</p>
  {/each}
</Page>

<style>
  .row { display: flex; align-items: center; gap: 12px; padding: 10px 4px; border-bottom: 1px solid var(--border); }
  .kind { font-size: 11px; width: 70px; color: var(--fg-muted); }
  .body { flex: 1; }
  .value { font-weight: 550; }
</style>
