<script lang="ts">
  import Icon from "../Icon.svelte";
  import { t, type Key } from "../i18n.svelte";
  import { app, type SettingsSection } from "../state.svelte";
  import About from "./About.svelte";
  import Audio from "./Audio.svelte";
  import Developer from "./Developer.svelte";
  import Documents from "./Documents.svelte";
  import General from "./General.svelte";
  import Llm from "./Llm.svelte";
  import Models from "./Models.svelte";
  import Recognition from "./Recognition.svelte";
  import Speakers from "./Speakers.svelte";
  import Watch from "./Watch.svelte";

  type Item = { id: SettingsSection; icon: string; title: Key };
  const groups: { title: Key | null; items: Item[] }[] = $derived([
    { title: null, items: [{ id: "general", icon: "sliders", title: "set.general" }] },
    {
      title: "set.group.processing",
      items: [
        { id: "recognition", icon: "mic", title: "set.asr" },
        { id: "speakers", icon: "people", title: "set.diar" },
        { id: "audio", icon: "volume", title: "set.audio" },
        { id: "llm", icon: "sparkle", title: "set.llm" },
      ],
    },
    {
      title: "set.group.result",
      items: [
        { id: "documents", icon: "doc", title: "set.docs" },
        { id: "watch", icon: "inbox", title: "set.watch" },
      ],
    },
    {
      title: "set.group.app",
      items: [
        { id: "models", icon: "box", title: "set.models" },
        ...(app.developer ? [{ id: "developer", icon: "code", title: "set.dev" } satisfies Item] : []),
        { id: "about", icon: "info", title: "set.about" },
      ],
    },
  ]);
  const current = $derived(groups.flatMap((g) => g.items).find((i) => i.id === app.section) ?? groups[0].items[0]);
  let body = $state<HTMLElement>();
  $effect(() => {
    current.id;
    body?.scrollTo({ top: 0 });
  });
</script>

<div class="settings">
  <nav aria-label={t("nav.settings")}>
    <h1>{t("nav.settings")}</h1>
    {#each groups as g (g.title)}
      {#if g.title}<div class="label">{t(g.title)}</div>{/if}
      {#each g.items as i (i.id)}
        <button class:active={current.id === i.id} aria-current={current.id === i.id} onclick={() => (app.section = i.id)}>
          <Icon name={i.icon} size={15} /> {t(i.title)}
        </button>
      {/each}
    {/each}
  </nav>
  <div class="body" bind:this={body}>
    {#if app.settings && app.info}
      <div class="content">
        {#if current.id !== "about"}<h1>{t(current.title)}</h1>{/if}
        {#if current.id === "general"}<General />
        {:else if current.id === "recognition"}<Recognition />
        {:else if current.id === "speakers"}<Speakers />
        {:else if current.id === "audio"}<Audio />
        {:else if current.id === "llm"}<Llm />
        {:else if current.id === "documents"}<Documents />
        {:else if current.id === "watch"}<Watch />
        {:else if current.id === "models"}<Models />
        {:else if current.id === "developer"}<Developer />
        {:else}<About />{/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .settings { display: grid; grid-template-columns: 216px minmax(0, 1fr); height: 100vh; min-height: 0; }
  nav { padding: 38px 10px 16px; border-right: 1px solid var(--border); overflow-y: auto; }
  nav h1 { margin: 0 8px 14px; }
  .label {
    padding: 14px 8px 4px; font-size: var(--fs-xs); font-weight: 600; color: var(--fg-faint);
    text-transform: uppercase; letter-spacing: 0.04em;
  }
  nav button {
    display: flex; align-items: center; gap: 9px; width: 100%; text-align: left; border: none; background: transparent;
    padding: 6px 8px; border-radius: var(--r-md); margin-bottom: 1px;
  }
  nav button:hover { background: var(--bg-hover); }
  nav button.active { background: var(--bg-selected); color: var(--accent-text); font-weight: 550; }
  .body { overflow-y: auto; min-height: 0; }
  .content { max-width: 700px; padding: 38px 32px 48px; }
  .content > h1 { margin: 0 2px 20px; }
</style>
