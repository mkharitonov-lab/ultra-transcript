<script lang="ts">
  import { api } from "../api";
  import { t } from "../i18n.svelte";
  import { app } from "../state.svelte";
  import Group from "../ui/Group.svelte";
  import Row from "../ui/Row.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import ModelChoice from "./ModelChoice.svelte";

  const s = $derived(app.settings!);
  const save = () => app.saveSettings();
  const llms = $derived(app.info?.models.filter((m) => m.kind === "llm") ?? []);

  let test = $state<{ state: "idle" | "busy" | "ok" | "error"; text: string }>({ state: "idle", text: "" });
  async function check() {
    test = { state: "busy", text: "" };
    test = await api.testLlm($state.snapshot(s)).then(
      () => ({ state: "ok" as const, text: t("set.llm.works") }),
      (e) => ({ state: "error" as const, text: String(e) }),
    );
  }
  function changed() {
    test = { state: "idle", text: "" };
    save();
  }

  const presets = [
    { name: "Ollama", url: "http://localhost:11434/v1", model: "qwen3:8b" },
    { name: "LM Studio", url: "http://localhost:1234/v1", model: "" },
  ];
</script>

<Group>
  <Row label={t("set.llm.enable")} hint={t("set.llm.enableHint")}>
    <Toggle bind:checked={s.llm_enabled} onchange={changed} label={t("set.llm.enable")} />
  </Row>
</Group>

{#if s.llm_enabled}
  <Group title={t("set.llm.where")} bare>
    <Segmented bind:value={s.llm_provider} onchange={changed} options={[
      { value: "builtin", label: t("set.llm.builtin") },
      { value: "api", label: t("set.llm.api") },
    ]} />
    <p class="muted note">{t(s.llm_provider === "builtin" ? "set.llm.builtinHint" : "set.llm.apiHint")}</p>

    {#if s.llm_provider === "builtin"}
      <ModelChoice bind:value={s.llm_local_model} onchange={changed} options={llms.map((m) => ({
        value: m.name, title: m.title, model: m,
        badge: m.name === "gigachat-lightning" ? t("common.recommended") : m.name === "qwen3-4b" ? t("set.llm.light") : undefined,
      }))} />
    {:else}
      <div class="rows">
        <Row label={t("set.llm.presets")}>
          {#each presets as p (p.name)}
            <button onclick={() => { s.llm_base_url = p.url; if (p.model) s.llm_model = p.model; changed(); }}>{p.name}</button>
          {/each}
        </Row>
        <Row label={t("set.llm.url")}>
          <input type="text" class="wide" bind:value={s.llm_base_url} onchange={changed} spellcheck="false" placeholder="https://…/v1" />
        </Row>
        <Row label={t("set.llm.model")}>
          <input type="text" class="wide" bind:value={s.llm_model} onchange={changed} spellcheck="false" placeholder="qwen3:8b" />
        </Row>
        <Row label={t("set.llm.key")} hint={t("set.llm.keyHint")}>
          <input type="password" class="wide" bind:value={s.llm_api_key} onchange={changed} />
        </Row>
      </div>
    {/if}

    <div class="check">
      <button onclick={check} disabled={test.state === "busy"}>
        {#if test.state === "busy"}<span class="spinner"></span>{/if}{t("set.llm.check")}
      </button>
      <span class="result {test.state}">{test.text}</span>
    </div>
  </Group>

  <Group title={t("set.llm.directories")}>
    <Row label={t("set.llm.auto")} hint={t("set.llm.autoHint")}>
      <Toggle bind:checked={s.auto_accept_suggestions} onchange={save} label={t("set.llm.auto")} />
    </Row>
  </Group>
{/if}

<style>
  .note { margin: 8px 2px 10px; font-size: var(--fs-sm); max-width: 620px; }
  .rows { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--bg-card); overflow: hidden; }
  .rows > :global(* + *) { border-top: 1px solid var(--border); }
  .wide { width: 300px; }
  .check { display: flex; align-items: center; gap: 10px; margin-top: 12px; }
  .check button { display: inline-flex; align-items: center; gap: 7px; }
  .result { font-size: var(--fs-sm); user-select: text; -webkit-user-select: text; overflow-wrap: anywhere; }
  .result.ok { color: var(--ok); }
  .result.error { color: var(--danger); }
</style>
