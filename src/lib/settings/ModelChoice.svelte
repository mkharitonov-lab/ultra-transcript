<script lang="ts" generics="T extends string">
  import type { Model } from "../api";
  import { fmtSize, t } from "../i18n.svelte";
  import { models } from "../models.svelte";
  import Progress from "../ui/Progress.svelte";

  type Option = {
    value: T;
    title: string;
    about: string;
    /** Модель, которую нужно скачать, чтобы выбрать вариант; нет — скачивать нечего. */
    model?: Model | null;
    badge?: string;
    /** Почему вариант недоступен. */
    blocked?: string;
  };

  /** Выбор одного варианта из нескольких; нескачанную модель сначала предлагают скачать. */
  let { value = $bindable(), options, onchange }: { value: T; options: Option[]; onchange?: (v: T) => void } = $props();

  function pick(o: Option) {
    if (value === o.value || o.blocked) return;
    value = o.value;
    onchange?.(o.value);
  }
  const needs = (o: Option) => !!o.model && !o.model.installed;
</script>

<div class="choice" role="radiogroup">
  {#each options as o (o.value)}
    {@const loading = o.model ? models.progress(o.model.name) : undefined}
    <div class="option" class:on={value === o.value} class:off={!!o.blocked}>
      <button class="pick" role="radio" aria-checked={value === o.value} disabled={!!o.blocked || needs(o)} onclick={() => pick(o)}>
        <span class="dot"></span>
        <span class="text">
          <span class="title">{o.title}{#if o.badge}<span class="badge">{o.badge}</span>{/if}</span>
          <span class="about muted">{o.about}</span>
          {#if o.blocked}<span class="about faint">{o.blocked}</span>{/if}
        </span>
      </button>
      {#if o.model && !o.blocked}
        <div class="state">
          {#if loading !== undefined}
            <Progress value={loading} width="96px" />
          {:else if needs(o)}
            <button onclick={() => models.download([o.model!.name], () => pick(o))}>{t("common.download")} · {fmtSize(o.model.size_mb)}</button>
          {/if}
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .choice { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--bg-card); overflow: hidden; }
  .option { display: flex; align-items: center; gap: 12px; padding-right: 14px; }
  .option + .option { border-top: 1px solid var(--border); }
  .option.on { background: var(--bg-selected); }
  .option.off { opacity: 0.6; }
  .pick {
    flex: 1; display: flex; align-items: flex-start; gap: 11px; text-align: left; white-space: normal;
    border: none; background: transparent; border-radius: 0; padding: 11px 0 11px 14px; min-width: 0;
  }
  .pick:hover { background: transparent; }
  .pick:disabled { opacity: 1; }
  .dot {
    width: 16px; height: 16px; margin-top: 1px; border-radius: 50%; border: 1.5px solid var(--border-strong);
    background: var(--bg-input); flex-shrink: 0; display: grid; place-items: center;
  }
  .on .dot { border-color: var(--accent); background: var(--accent); }
  .on .dot::after { content: ""; width: 6px; height: 6px; border-radius: 50%; background: #fff; }
  .text { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .title { font-weight: 550; display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .about { font-size: var(--fs-sm); }
  .badge {
    font-size: var(--fs-xs); font-weight: 500; background: var(--bg-selected); color: var(--accent);
    border-radius: var(--r-sm); padding: 0 6px;
  }
  .on .badge { background: var(--bg-elevated); }
  .state { flex-shrink: 0; }
</style>
