<script lang="ts">
  import Icon from "./Icon.svelte";
  import ModelsCard from "./ModelsCard.svelte";
  import type { AppInfo } from "./api";

  let { info, onadd, onmodels }: { info: AppInfo; onadd: () => void; onmodels: () => void } = $props();
  const ready = $derived(info.models.every((m) => m.installed));
</script>

<div class="home">
  {#if !ready}
    <div class="intro">
      <h1>Добро пожаловать</h1>
      <p class="muted">Для работы нужно один раз скачать модели распознавания речи и голосов.</p>
    </div>
    <ModelsCard {info} {onmodels} />
  {:else}
    <button class="drop" onclick={onadd}>
      <Icon name="wave" size={44} />
      <span class="big">Перетащите аудио или видео</span>
      <span class="muted">или нажмите, чтобы выбрать файлы — mp3, m4a, wav, mp4, mov и другие</span>
    </button>
    <p class="hint faint">Совет: в разделе «Папки» можно указать папку — новые записи из неё будут расшифровываться автоматически.</p>
  {/if}
</div>

<style>
  .home { flex: 1; display: flex; flex-direction: column; justify-content: center; padding: 40px max(40px, 8%); gap: 20px; }
  .intro p { margin: 6px 0 0; }
  .drop {
    display: flex; flex-direction: column; align-items: center; gap: 10px; padding: 70px 20px;
    border: 2px dashed var(--border); border-radius: 16px; background: transparent; color: var(--fg-muted);
  }
  .drop:hover { border-color: var(--accent); color: var(--accent); background: var(--bg-selected); }
  .big { font-size: 17px; font-weight: 600; color: var(--fg); }
  .hint { text-align: center; margin: 0; }
</style>
