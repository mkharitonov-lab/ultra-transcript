<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import Page from "./Page.svelte";
  import Icon from "./Icon.svelte";
  import { api, confirmDialog, showError, type Rule } from "./api";

  let rules = $state<Rule[]>([]);
  onMount(async () => (rules = await api.rules()));

  const dirs: [keyof Rule, string, string][] = [
    ["input_dir", "Следить за папкой", "Новые аудио и видео отсюда расшифровываются автоматически"],
    ["audio_dir", "Сжатое аудио", "Архив записи в Opus (~11 МБ/час)"],
    ["transcript_dir", "Расшифровки", "Очищенный текст по спикерам, .docx"],
    ["protocol_dir", "Протоколы", "Протокол договорённостей по шаблону, .docx"],
  ];

  async function pick(r: Rule, key: keyof Rule) {
    const d = await open({ directory: true, defaultPath: (r[key] as string) || undefined });
    if (typeof d === "string") {
      (r as Record<string, unknown>)[key] = d;
      await save(r);
    }
  }
  async function save(r: Rule) {
    if (!r.input_dir) return;
    r.id = await api.saveRule($state.snapshot(r)).catch((e) => (showError(e), r.id));
  }
  function add() {
    rules.push({ id: null, input_dir: "", audio_dir: "", transcript_dir: "", protocol_dir: "", protocol: true, enabled: true });
  }
  async function remove(r: Rule) {
    if (r.id && !(await confirmDialog("Перестать следить за этой папкой?"))) return;
    if (r.id) await api.deleteRule(r.id);
    rules = rules.filter((x) => x !== r);
  }
</script>

<Page title="Папки" subtitle="Приложение следит за папками в фоне — даже когда окно закрыто (иконка в строке меню). Когда файл полностью скопирован, он расшифровывается, а результаты раскладываются по папкам.">
  {#snippet actions()}<button class="primary" onclick={add}><Icon name="plus" size={12} /> Папка</button>{/snippet}
  {#each rules as r (r)}
    <div class="card" class:off={!r.enabled}>
      {#each dirs as [key, label, hint]}
        <div class="row">
          <div class="label"><div>{label}</div><div class="faint small">{hint}</div></div>
          <button class="path" onclick={() => pick(r, key)} title={r[key] as string}>
            <Icon name="folder" size={13} />
            <span>{(r[key] as string) || (key === "input_dir" ? "Выбрать папку…" : "Не сохранять отдельно")}</span>
          </button>
          {#if key !== "input_dir" && r[key]}
            <button class="ghost" title="Не сохранять отдельно" onclick={() => { (r as Record<string, unknown>)[key] = ""; save(r); }}><Icon name="x" size={12} /></button>
          {/if}
        </div>
      {/each}
      <div class="foot">
        <label><input type="checkbox" bind:checked={r.protocol} onchange={() => save(r)} /> Составлять протокол (нужна LLM)</label>
        <label><input type="checkbox" bind:checked={r.enabled} onchange={() => save(r)} /> Включено</label>
        <span class="spacer"></span>
        <button class="ghost danger" onclick={() => remove(r)}><Icon name="trash" /> Удалить</button>
      </div>
    </div>
  {:else}
    <p class="empty-state">Нет отслеживаемых папок. Например, укажите папку, куда диктофон или Zoom сохраняет записи.</p>
  {/each}
</Page>

<style>
  .card { border: 1px solid var(--border); border-radius: 12px; padding: 12px 16px; margin-bottom: 14px; background: var(--bg-card); }
  .card.off { opacity: 0.6; }
  .row { display: flex; align-items: center; gap: 12px; padding: 6px 0; }
  .label { width: 240px; flex-shrink: 0; }
  .small { font-size: 11px; }
  .path { flex: 1; display: flex; align-items: center; gap: 6px; text-align: left; overflow: hidden; }
  .path span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; direction: rtl; text-align: left; }
  .foot { display: flex; align-items: center; gap: 18px; border-top: 1px solid var(--border); margin-top: 8px; padding-top: 10px; }
  .foot label { display: flex; align-items: center; gap: 6px; }
  .spacer { flex: 1; }
</style>
