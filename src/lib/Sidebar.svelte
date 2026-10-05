<script lang="ts">
  import Icon from "./Icon.svelte";
  import { api, fmtTime, type Folder, type Recording } from "./api";
  import { fmtDate, i18n, locale, t, tn } from "./i18n.svelte";
  import { live } from "./live.svelte";
  import { folderMenu, newFolder, recordingMenu, remove, startMeeting, startRecording } from "./recordings";
  import { app, type View } from "./state.svelte";
  import { openMenu, type MenuItem } from "./ui/menu.svelte";

  let { onadd }: { onadd: () => void } = $props();
  /** Запись с микрофона, которая идёт сейчас, — кнопка «Записать» ведёт к ней. */
  const capturing = $derived(live.recording);

  let query = $state("");
  /** Совпадения в тексте расшифровок: запись → фрагмент с найденным. */
  let hits = $state<Record<string, string>>({});
  let searchField = $state<HTMLInputElement>();
  /** Выделенные записи (⌘-щелчок, ⇧-щелчок) — для действий сразу над несколькими. */
  let marked = $state<string[]>([]);
  let anchor = $state<string | null>(null);

  // Свёрнутые папки и архив запоминаются между запусками.
  const STORE = "library.collapsed";
  let collapsed = $state<Record<string, boolean>>(JSON.parse(localStorage.getItem(STORE) ?? '{"archive":true}'));
  function toggle(key: string) {
    collapsed[key] = !collapsed[key];
    localStorage.setItem(STORE, JSON.stringify(collapsed));
  }

  // Ушли со страницы записи — выделение больше не нужно.
  $effect(() => {
    if (app.view !== "recording") marked = [];
  });

  const q = $derived(query.trim().toLowerCase());
  $effect(() => {
    const text = q;
    if (text.length < 2) {
      hits = {};
      return;
    }
    const timer = setTimeout(async () => {
      const found = await api.search(text).catch(() => []);
      if (text === q) hits = Object.fromEntries(found.map((h) => [h.id, h.snippet]));
    }, 200);
    return () => clearTimeout(timer);
  });

  const found = $derived(app.recordings.filter((r) => r.title.toLowerCase().includes(q) || r.id in hits));
  const active = $derived(app.recordings.filter((r) => !r.archived));
  const archived = $derived(app.recordings.filter((r) => r.archived));
  const inFolder = (f: Folder) => active.filter((r) => r.folder_id === f.id);
  const loose = $derived(active.filter((r) => r.folder_id === null || !app.folders.some((f) => f.id === r.folder_id)));

  /** Записи вне папок — по времени: сегодня, вчера, неделя, месяц, дальше по месяцам. */
  const groups = $derived.by(() => {
    i18n.lang;
    const now = new Date();
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
    const out: { label: string; items: Recording[] }[] = [];
    for (const r of loose) {
      const d = new Date(r.created_at.replace(" ", "T"));
      const days = Math.floor((today - new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()) / 86_400_000);
      let label: string;
      if (isNaN(days) || days <= 0) label = t("date.today");
      else if (days === 1) label = t("date.yesterday");
      else if (days <= 7) label = t("date.week");
      else if (days <= 30) label = t("date.month");
      else {
        const m = d.toLocaleDateString(locale(), { month: "long", year: d.getFullYear() === now.getFullYear() ? undefined : "numeric" });
        label = m[0].toUpperCase() + m.slice(1);
      }
      if (out[out.length - 1]?.label === label) out[out.length - 1].items.push(r);
      else out.push({ label, items: [r] });
    }
    return out;
  });

  /** Записи в том порядке, в каком они видны в списке, — для выделения диапазона и стрелок. */
  const visible = $derived.by(() => {
    if (q) return found;
    const out: Recording[] = [];
    for (const f of app.folders) if (!collapsed[`f${f.id}`]) out.push(...inFolder(f));
    out.push(...loose);
    if (!collapsed.archive) out.push(...archived);
    return out;
  });

  function click(e: MouseEvent, r: Recording) {
    if (e.metaKey || e.ctrlKey) {
      const base = marked.length ? marked : app.selected ? [app.selected] : [];
      marked = base.includes(r.id) ? base.filter((id) => id !== r.id) : [...base, r.id];
      anchor = r.id;
    } else if (e.shiftKey && (anchor ?? app.selected)) {
      const order = visible.map((x) => x.id);
      const [a, b] = [order.indexOf((anchor ?? app.selected)!), order.indexOf(r.id)].sort((x, y) => x - y);
      marked = a < 0 ? [r.id] : order.slice(a, b + 1);
    } else {
      marked = [];
      anchor = r.id;
      app.open(r.id);
    }
  }

  function targets(r: Recording): Recording[] {
    if (!marked.includes(r.id)) {
      marked = [];
      return [r];
    }
    return app.recordings.filter((x) => marked.includes(x.id));
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const order = visible.map((x) => x.id);
      const i = order.indexOf(app.selected ?? "");
      const next = order[Math.max(0, Math.min(order.length - 1, i < 0 ? 0 : i + (e.key === "ArrowDown" ? 1 : -1)))];
      if (!next) return;
      marked = [];
      anchor = next;
      app.open(next);
      e.preventDefault();
      requestAnimationFrame(() => document.querySelector(`.library [data-id="${next}"]`)?.scrollIntoView({ block: "nearest" }));
    } else if ((e.key === "Backspace" || e.key === "Delete") && (e.metaKey || e.ctrlKey)) {
      const rs = marked.length ? app.recordings.filter((x) => marked.includes(x.id)) : app.current ? [app.current] : [];
      if (rs.length) remove(rs);
      e.preventDefault();
    } else if (e.key === "Escape") marked = [];
  }

  function shortcut(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      searchField?.focus();
      searchField?.select();
    }
  }

  const mac = navigator.platform.toLowerCase().includes("mac");
  /** Что записать: микрофон или видеовстречу (микрофон и звук компьютера). */
  const recordMenu: MenuItem[] = $derived([
    { label: t("side.recordMic"), icon: "mic", hint: mac ? "⌘R" : "Ctrl+R", action: () => startRecording() },
    { label: t("side.meeting"), icon: "video", hint: mac ? "⇧⌘R" : "Ctrl+Shift+R", action: startMeeting },
  ]);

  const nav: { view: Exclude<View, "recording" | "home">; key: "nav.dictionary" | "nav.people" | "nav.settings"; icon: string }[] = [
    { view: "dictionary", key: "nav.dictionary", icon: "book" },
    { view: "people", key: "nav.people", icon: "people" },
    { view: "settings", key: "nav.settings", icon: "gear" },
  ];
  const toReview = (v: View) => (v === "dictionary" ? app.pending.terms : v === "people" ? app.pending.people : 0);
</script>

<svelte:window onkeydown={shortcut} />

{#snippet row(r: Recording)}
  {@const job = live.of(r.id)}
  {@const capturing = r.status === "recording"}
  {@const working = !capturing && (r.status === "processing" || !!job)}
  <button class="item rec" data-id={r.id}
    class:active={app.selected === r.id && !marked.length} class:marked={marked.includes(r.id)} class:dim={r.archived}
    onclick={(e) => click(e, r)}
    oncontextmenu={(e) => openMenu(e, recordingMenu(targets(r)))}>
    <span class="status {working ? 'processing' : r.status}">
      {#if capturing}<span class="reddot"></span>
      {:else if working}<span class="spinner"></span>
      {:else if r.status === "queued"}<Icon name="clock" size={13} />
      {:else if r.status === "error"}<Icon name="alert" size={13} />
      {:else}<Icon name="wave" size={13} />{/if}
    </span>
    <span class="body">
      <span class="title">{r.title}</span>
      <span class="meta">
        {#if q && hits[r.id]}…{hits[r.id]}…
        {:else if capturing}{t("status.recording")} · {fmtTime(job?.seconds ?? 0)}
        {:else if working}{job?.title || t("status.processing")}{job && job.progress > 0 ? ` · ${Math.round(job.progress * 100)}%` : ""}
        {:else if r.status === "queued"}{t("status.queued")}
        {:else if r.status === "error"}{t("status.error")}
        {:else}{fmtDate(r.created_at)} · {fmtTime(r.duration)}{/if}
      </span>
    </span>
  </button>
{/snippet}

<aside>
  <div class="titlebar" data-tauri-drag-region></div>
  <div class="top">
    <div class="actions">
      <div class="split">
        <button class="primary add" class:live={!!capturing} onclick={() => startRecording()}>
          {#if capturing}<span class="reddot"></span> {fmtTime(capturing.seconds)}{:else}<Icon name="mic" /> {t("side.record")}{/if}
        </button>
        {#if !capturing}
          <button class="primary more" aria-label={t("side.recordMore")} title={t("side.recordMore")} onclick={(e) => openMenu(e, recordMenu)}>
            <Icon name="chevron" size={12} />
          </button>
        {/if}
      </div>
      <!-- Без значка: рядом с кнопкой записи и её меню для «Добавить файлы» мало места. -->
      <button class="add" onclick={onadd}>{t("side.add")}</button>
    </div>
    <label class="search">
      <Icon name="search" size={13} />
      <input type="text" placeholder={t("side.search")} bind:value={query} bind:this={searchField} spellcheck="false"
        onkeydown={(e) => { if (e.key === "Escape") { query = ""; e.currentTarget.blur(); } }} />
      {#if query}<button class="ghost clear" onclick={() => (query = "")} aria-label={t("common.reset")}><Icon name="x" size={11} /></button>{/if}
    </label>
    <button class="item nav home" class:active={app.view === "home"} onclick={() => app.go("home")}>
      <Icon name="home" size={15} /> <span>{t("nav.home")}</span>
    </button>
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <nav class="library" {onkeydown} oncontextmenu={(e) => openMenu(e, [
    { label: t("side.record"), icon: "mic", action: () => startRecording() },
    { label: t("side.meeting"), icon: "video", action: startMeeting },
    { label: t("side.add"), icon: "plus", action: onadd },
    { label: t("folder.new"), icon: "folder-plus", action: () => newFolder() },
  ])}>
    {#if q}
      <div class="label">{tn("side.found", found.length)}</div>
      {#each found as r (r.id)}{@render row(r)}{/each}
    {:else}
      <div class="label">
        <span>{t("side.library")}</span>
        <button class="ghost mini" onclick={() => newFolder()} title={t("folder.new")} aria-label={t("folder.new")}><Icon name="folder-plus" size={13} /></button>
      </div>

      {#each app.folders as f (f.id)}
        {@const items = inFolder(f)}
        <button class="item folder" aria-expanded={!collapsed[`f${f.id}`]} onclick={() => toggle(`f${f.id}`)}
          oncontextmenu={(e) => openMenu(e, folderMenu(f))}>
          <span class="twist" class:open={!collapsed[`f${f.id}`]}><Icon name="chevron-right" size={11} /></span>
          <Icon name="folder" size={14} />
          <span class="title">{f.name}</span>
          <span class="count">{items.length || ""}</span>
        </button>
        {#if !collapsed[`f${f.id}`]}
          <div class="nested">
            {#each items as r (r.id)}{@render row(r)}{:else}<div class="hint faint">{t("folder.empty")}</div>{/each}
          </div>
        {/if}
      {/each}

      {#each groups as g (g.label)}
        <div class="label sub">{g.label}</div>
        {#each g.items as r (r.id)}{@render row(r)}{/each}
      {/each}

      {#if !active.length}
        <div class="hint faint">{t(app.recordings.length ? "side.allArchived" : "side.empty")}</div>
      {/if}

      {#if archived.length}
        <button class="item folder archive" aria-expanded={!collapsed.archive} onclick={() => toggle("archive")}>
          <span class="twist" class:open={!collapsed.archive}><Icon name="chevron-right" size={11} /></span>
          <Icon name="archive" size={14} />
          <span class="title">{t("side.archive")}</span>
          <span class="count">{archived.length}</span>
        </button>
        {#if !collapsed.archive}
          <div class="nested">{#each archived as r (r.id)}{@render row(r)}{/each}</div>
        {/if}
      {/if}
    {/if}
  </nav>

  <nav class="bottom">
    {#each nav as n (n.view)}
      <button class="item nav" class:active={app.view === n.view} onclick={() => app.go(n.view)}>
        <Icon name={n.icon} size={15} /> <span>{t(n.key)}</span>
        {#if toReview(n.view)}<span class="badge" title={tn("side.toReview", toReview(n.view))}>{toReview(n.view)}</span>{/if}
      </button>
    {/each}
  </nav>
</aside>

<style>
  aside {
    background: var(--bg-sidebar); border-right: 1px solid var(--border);
    display: flex; flex-direction: column; min-height: 0; min-width: 0;
  }
  .titlebar { height: 38px; flex-shrink: 0; }
  .top { padding: 0 12px 2px; display: flex; flex-direction: column; gap: 8px; }
  .home { margin: 0 -4px; width: auto; }
  .actions { display: flex; gap: 4px; }
  .add { flex: 1; min-width: 0; display: flex; align-items: center; justify-content: center; gap: 5px; padding: 6px 6px; font-weight: 550; white-space: nowrap; overflow: hidden; }
  .add :global(svg) { flex-shrink: 0; }
  /* Кнопка записи — по содержимому: длинной подписи «Добавить файлы» нужно больше места. */
  .add.primary { flex: 0 0 auto; }
  .split { display: flex; }
  .split:has(.more) .add { border-top-right-radius: 0; border-bottom-right-radius: 0; }
  .more { display: flex; align-items: center; padding: 6px 3px; border-top-left-radius: 0; border-bottom-left-radius: 0; border-left: 1px solid color-mix(in srgb, var(--accent-fg) 25%, transparent); }
  .add.live { background: var(--danger); font-variant-numeric: tabular-nums; }
  .add.live .reddot { background: var(--accent-fg); }
  .reddot { width: 9px; height: 9px; border-radius: 50%; background: var(--danger); animation: pulse 1.4s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: 0.35; } }
  .search {
    display: flex; align-items: center; gap: 6px; padding: 0 8px; border-radius: var(--r-sm);
    background: var(--bg-input); border: 1px solid var(--border); color: var(--fg-faint);
  }
  .search:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px var(--bg-selected); }
  .search input { border: none; background: transparent; box-shadow: none; padding: 5px 0; color: var(--fg); }
  .clear { padding: 3px; display: grid; border-radius: 50%; }

  .library { flex: 1; overflow-y: auto; padding: 0 8px 8px; min-height: 0; outline: none; }
  .bottom { padding: 8px; border-top: 1px solid var(--border); }
  .label {
    display: flex; align-items: center; justify-content: space-between; padding: 10px 8px 4px;
    font-size: var(--fs-xs); font-weight: 600; color: var(--fg-faint); text-transform: uppercase; letter-spacing: 0.04em;
  }
  .label.sub { text-transform: none; letter-spacing: 0; padding-top: 10px; }
  .mini { padding: 3px; display: grid; color: var(--fg-muted); margin: -3px -2px; }
  .hint { padding: 6px 8px 8px; font-size: var(--fs-sm); }

  .item {
    display: flex; align-items: center; gap: 9px; width: 100%; text-align: left;
    border: none; background: transparent; padding: 6px 8px; border-radius: var(--r-md); margin-bottom: 1px;
  }
  .item:hover { background: var(--bg-hover); }
  .item.active { background: var(--bg-selected); }
  .item.marked { background: var(--accent); color: var(--accent-fg); }
  .item.marked .meta, .item.marked .status { color: inherit; }
  .item.marked .status { background: transparent; border-color: color-mix(in srgb, currentColor 35%, transparent); }
  .item.nav, .item.folder { padding: 5px 8px; gap: 8px; }
  .item.nav span, .item.folder .title { flex: 1; }
  .item.dim { opacity: 0.65; }
  .folder .title { font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .archive { margin-top: 10px; color: var(--fg-muted); }
  .twist { display: grid; color: var(--fg-faint); transition: transform 0.12s; margin: 0 -3px 0 -2px; }
  .twist.open { transform: rotate(90deg); }
  .count { color: var(--fg-faint); font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
  .nested { padding-left: 14px; }

  .body { display: flex; flex-direction: column; min-width: 0; flex: 1; }
  .title { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-weight: 500; }
  .meta { font-size: var(--fs-xs); color: var(--fg-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .status {
    width: 24px; height: 24px; border-radius: var(--r-sm); display: grid; place-items: center; flex-shrink: 0;
    background: var(--bg-card); color: var(--fg-muted); border: 1px solid var(--border);
  }
  .status.done { color: var(--accent); }
  .status.recording { color: var(--danger); }
  .status.error { color: var(--danger); }
  .badge {
    flex: 0 !important; background: var(--warn); color: var(--warn-fg); border-radius: 9px; padding: 0 6px;
    font-size: var(--fs-xs); font-weight: 600;
  }
</style>
