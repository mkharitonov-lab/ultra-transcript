<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import Page from "./Page.svelte";
  import ModelsCard from "./ModelsCard.svelte";
  import { api, showError, type AppInfo, type AsrModel, type Settings } from "./api";

  let { info, onmodels }: { info: AppInfo; onmodels: () => void } = $props();
  let s = $state<Settings | null>(null);
  let test = $state("");
  let saved = $state(false);
  let downloading = $state<Record<string, number>>({});

  const asrModels: { id: AsrModel; name: string; about: string }[] = [
    { id: "gigaam", name: "GigaAM v3", about: "Сбер. Лучший выбор для русского: точнее и в несколько раз быстрее, сама расставляет пунктуацию и пишет числа цифрами." },
    { id: "whisper-turbo", name: "Whisper large-v3-turbo", about: "OpenAI. Многоязычная — для записей с английским и другими языками. Медленнее, иногда «додумывает» текст на шуме." },
  ];
  const modelFor = (id: AsrModel) => info.models.find((m) => m.asr === id);

  onMount(() => {
    api.settings().then((v) => (s = v));
    const un = api.onModels((e) => {
      if (!(e.name in downloading)) return;
      if (e.error) { showError(e.error); delete downloading[e.name]; return; }
      downloading[e.name] = e.progress;
      if (e.progress >= 1) { delete downloading[e.name]; onmodels(); }
    });
    return () => { un.then((f) => f()); };
  });

  function download(name: string) {
    downloading[name] = 0;
    api.installModels([name]);
  }

  async function save() {
    if (!s) return;
    await api.saveSettings($state.snapshot(s)).catch(showError);
    saved = true;
    setTimeout(() => (saved = false), 1200);
  }
  async function check() {
    if (!s) return;
    test = "Проверка…";
    test = await api.testLlm($state.snapshot(s)).then(() => "✓ Работает").catch((e) => `✗ ${e}`);
  }
  async function pickTemplate(key: "transcript_template" | "protocol_template") {
    const f = await open({ filters: [{ name: "Word", extensions: ["docx"] }], defaultPath: info.templates_dir });
    if (s && typeof f === "string") {
      s[key] = f;
      save();
    }
  }
  const presets = [
    ["Ollama", "http://localhost:11434/v1", "qwen3:8b"],
    ["LM Studio", "http://localhost:1234/v1", ""],
    ["OpenAI-совместимый API", "https://", ""],
  ];
</script>

<Page title="Настройки">
  {#snippet actions()}{#if saved}<span class="muted">Сохранено</span>{/if}{/snippet}
  {#if s}
    <section>
      <h2>Распознавание речи</h2>
      <div class="asr">
        {#each asrModels as a}
          {@const m = modelFor(a.id)}
          <label class="asr-card" class:on={s.asr_model === a.id}>
            <input type="radio" name="asr" value={a.id} bind:group={s.asr_model} onchange={save} />
            <div class="asr-body">
              <div class="asr-name">{a.name}
                {#if m?.installed}<span class="ok">установлена</span>{/if}
              </div>
              <div class="muted">{a.about}</div>
              {#if m && !m.installed}
                {#if m.name in downloading}
                  <div class="bar"><span style="width:{Math.round(downloading[m.name] * 100)}%"></span></div>
                {:else}
                  <button onclick={(e) => { e.preventDefault(); download(m.name); }}>Скачать · {m.size_mb} МБ</button>
                {/if}
              {/if}
            </div>
          </label>
        {/each}
      </div>
      <p class="faint small">Модель применяется к новым записям. Любую запись можно расшифровать заново другой моделью — кнопка ↻ в заголовке записи.</p>
    </section>

    <section>
      <h2>Языковая модель (LLM)</h2>
      <p class="muted desc">Исправляет термины и ФИО по справочникам, убирает слова-паразиты, пополняет справочники и составляет протоколы. Локальная (Ollama, LM Studio) или любой OpenAI-совместимый endpoint.</p>
      <label class="check"><input type="checkbox" bind:checked={s.llm_enabled} onchange={save} /> Использовать LLM</label>
      <div class="presets">
        {#each presets as [name, url, model]}
          <button class="ghost" onclick={() => { s!.llm_base_url = url; if (model) s!.llm_model = model; save(); }}>{name}</button>
        {/each}
      </div>
      <div class="form">
        <span class="lbl">Адрес API (base URL)</span><input type="text" bind:value={s.llm_base_url} onchange={save} />
        <span class="lbl">Модель</span><input type="text" bind:value={s.llm_model} onchange={save} placeholder="qwen3:8b" />
        <span class="lbl">API-ключ</span><input type="password" bind:value={s.llm_api_key} onchange={save} placeholder="не нужен для локальных" />
        <span></span><div><button onclick={check}>Проверить соединение</button> <span class="muted">{test}</span></div>
      </div>
      <label class="check"><input type="checkbox" bind:checked={s.auto_accept_suggestions} onchange={save} /> Добавлять новые термины и людей в справочники без подтверждения</label>
    </section>

    <section>
      <h2>Спикеры</h2>
      <div class="form">
        <span class="lbl">Разделение спикеров</span>
        <div class="slider"><span class="faint">меньше</span><input type="range" min="0.2" max="0.8" step="0.05" bind:value={s.cluster_threshold} onchange={save} style="direction:rtl" /><span class="faint">больше</span><span class="num">{s.cluster_threshold}</span></div>
        <span class="lbl">Порог узнавания голоса</span>
        <div class="slider"><span class="faint">мягче</span><input type="range" min="0.35" max="0.85" step="0.05" bind:value={s.voice_threshold} onchange={save} /><span class="faint">строже</span><span class="num">{s.voice_threshold}</span></div>
      </div>
    </section>

    <section>
      <h2>Файлы</h2>
      <div class="form">
        <span class="lbl">Битрейт архива (Opus)</span>
        <select bind:value={s.archive_kbps} onchange={save}>
          {#each [16, 24, 32, 48] as k}<option value={k}>{k} кбит/с · ≈{Math.round(k * 0.45)} МБ/час</option>{/each}
        </select>
        <span class="lbl">Шаблон расшифровки</span>
        <div class="tpl"><button onclick={() => pickTemplate("transcript_template")}>{s.transcript_template.split(/[\\/]/).pop() || "По умолчанию"}</button>
          {#if s.transcript_template}<button class="ghost" onclick={() => { s!.transcript_template = ""; save(); }}>Сбросить</button>{/if}</div>
        <span class="lbl">Шаблон протокола</span>
        <div class="tpl"><button onclick={() => pickTemplate("protocol_template")}>{s.protocol_template.split(/[\\/]/).pop() || "По умолчанию"}</button>
          {#if s.protocol_template}<button class="ghost" onclick={() => { s!.protocol_template = ""; save(); }}>Сбросить</button>{/if}</div>
        <span></span>
        <p class="muted small">Шаблон — обычный .docx. Метки <code>{"{{поле}}"}</code> заполняются автоматически; метка в пункте списка — повторяется для каждого элемента; <code>{"{{таблица.колонка}}"}</code> в строке таблицы — строка повторяется. Добавьте свою метку, например <code>{"{{риски}}"}</code>, — LLM заполнит и её.
          <button class="link" onclick={() => api.reveal(info.templates_dir, true)}>Открыть папку шаблонов</button></p>
      </div>
    </section>

    <section>
      <ModelsCard {info} {onmodels} asr={s.asr_model} />
      <p class="faint small">Данные: <button class="link" onclick={() => api.reveal(info.data_dir, true)}>{info.data_dir}</button></p>
    </section>
  {/if}
</Page>

<style>
  section { max-width: 760px; margin-bottom: 28px; }
  .asr { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin-bottom: 8px; }
  .asr-card { display: flex; gap: 10px; align-items: flex-start; border: 1px solid var(--border); border-radius: 10px; padding: 12px; background: var(--bg-card); }
  .asr-card.on { border-color: var(--accent); box-shadow: 0 0 0 3px var(--bg-selected); }
  .asr-card input { margin-top: 3px; accent-color: var(--accent); }
  .asr-body { display: flex; flex-direction: column; gap: 6px; align-items: flex-start; font-size: 12.5px; }
  .asr-name { font-weight: 600; font-size: 13px; display: flex; gap: 8px; align-items: center; }
  .ok { font-weight: 400; font-size: 11px; color: var(--ok); }
  .bar { width: 100%; height: 6px; border-radius: 3px; background: var(--border); overflow: hidden; }
  .bar span { display: block; height: 100%; background: var(--accent); transition: width 0.2s; }
  .desc { margin: -4px 0 10px; }
  .check { display: flex; align-items: center; gap: 8px; margin: 8px 0; }
  .presets { display: flex; gap: 4px; margin: 4px 0 8px; }
  .presets button { font-size: 12px; color: var(--accent); }
  .form { display: grid; grid-template-columns: 200px 1fr; gap: 8px 14px; align-items: center; }
  .form .lbl { color: var(--fg-muted); }
  .slider { display: flex; align-items: center; gap: 8px; }
  .slider input { flex: 1; accent-color: var(--accent); }
  .num { width: 36px; text-align: right; font-variant-numeric: tabular-nums; }
  .tpl { display: flex; gap: 6px; }
  .small { font-size: 12px; margin: 0; }
  code { background: var(--bg-hover); padding: 0 4px; border-radius: 4px; font-size: 11.5px; }
  .link { border: none; background: none; color: var(--accent); padding: 0; }
</style>
