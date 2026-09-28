<script lang="ts">
  import { onMount, tick } from "svelte";
  import Page from "./Page.svelte";
  import Icon from "./Icon.svelte";
  import { api, confirmDialog, showError, type Person } from "./api";

  let people = $state<Person[]>([]);
  let q = $state("");
  const shown = $derived(people.filter((p) => !q || [p.name, p.aliases, p.role, p.org].join(" ").toLowerCase().includes(q.toLowerCase())));

  onMount(async () => (people = await api.people()));

  async function save(p: Person) {
    if (!p.name.trim()) return;
    p.id = await api.savePerson($state.snapshot(p)).catch((e) => (showError(e), p.id));
  }
  async function add() {
    q = "";
    people.unshift({ id: null, name: "", aliases: "", role: "", org: "", voiceprints: 0 });
    await tick();
    (document.querySelector(".grid tbody input") as HTMLInputElement)?.focus();
  }
  async function forgetVoice(p: Person) {
    if (!p.id || !(await confirmDialog(`Удалить голосовой профиль: ${p.name}? Человек останется в справочнике.`))) return;
    await api.deleteVoiceprints(p.id);
    p.voiceprints = 0;
  }
  async function remove(p: Person) {
    if (p.id && !(await confirmDialog(`Удалить ${p.name} вместе с голосовым профилем?`))) return;
    if (p.id) await api.deletePerson(p.id);
    people = people.filter((x) => x !== p);
  }
</script>

<Page title="Кто есть кто" subtitle="ФИО пишутся в расшифровке и протоколе строго так. Голосовой профиль накапливается, когда вы указываете, кто говорит, — и дальше человек определяется автоматически. Профили хранятся только на этом компьютере в зашифрованном виде.">
  {#snippet actions()}
    <input type="search" placeholder="Поиск" bind:value={q} style="width:200px" />
    <button class="primary" onclick={add}><Icon name="plus" size={12} /> Человек</button>
  {/snippet}
  {#if people.length}
    <table class="grid">
      <thead><tr><th style="width:24%">ФИО</th><th style="width:18%">Обращения</th><th style="width:20%">Должность</th><th>Организация</th><th>Голос</th><th></th></tr></thead>
      <tbody>
        {#each shown as p (p)}
          <tr>
            <td><input type="text" bind:value={p.name} onchange={() => save(p)} placeholder="Петров Сергей Иванович" /></td>
            <td><input type="text" bind:value={p.aliases} onchange={() => save(p)} placeholder="Сергей Иванович, Серёжа" /></td>
            <td><input type="text" bind:value={p.role} onchange={() => save(p)} placeholder="Руководитель проекта" /></td>
            <td><input type="text" bind:value={p.org} onchange={() => save(p)} /></td>
            <td class="tools">
              {#if p.voiceprints}
                <button class="ghost voice" onclick={() => forgetVoice(p)} title="Удалить голосовой профиль">● {p.voiceprints} {p.voiceprints === 1 ? "запись" : "записи"}</button>
              {:else}<span class="faint">—</span>{/if}
            </td>
            <td class="tools"><button class="ghost" onclick={() => remove(p)} title="Удалить"><Icon name="trash" /></button></td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="empty-state">Справочник пуст. Добавьте людей вручную или просто укажите имя спикера в расшифровке.</p>
  {/if}
</Page>

<style>
  .voice { color: var(--ok); font-size: 12px; }
</style>
