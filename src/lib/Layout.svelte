<script lang="ts">
  import ContextMenu from "./ui/ContextMenu.svelte";
  import Dialogs from "./ui/Dialogs.svelte";
  import { openMenu } from "./ui/menu.svelte";
  import { textItems } from "./ui/textmenu";
  import Toasts from "./ui/Toasts.svelte";

  let { children } = $props();

  /**
   * Правый щелчок: системное меню WebView не показываем нигде. У записей, папок и строк таблиц —
   * свои меню; здесь — меню для текста: поля ввода, выделенный текст.
   */
  function oncontextmenu(e: MouseEvent) {
    if (e.defaultPrevented) return;
    e.preventDefault();
    openMenu(e, textItems(e));
  }
</script>

<svelte:window {oncontextmenu} />

{@render children()}
<ContextMenu />
<Dialogs />
<Toasts />

<style>
  :global(:root) {
    /* Поверхности */
    --bg: #ffffff;
    --bg-sidebar: #f4f4f6;
    --bg-card: #f8f8fa;
    --bg-elevated: #ffffff;
    --bg-input: #ffffff;
    --bg-hover: rgba(0, 0, 0, 0.05);
    --bg-active: rgba(0, 0, 0, 0.08);
    --bg-selected: rgba(91, 91, 214, 0.12);
    --scrim: rgba(20, 20, 30, 0.32);
    /* Текст */
    --fg: #1c1c1f;
    --fg-muted: #6b6b73;
    --fg-faint: #9a9aa2;
    /* Линии */
    --border: rgba(0, 0, 0, 0.09);
    --border-strong: rgba(0, 0, 0, 0.18);
    /* Акцент — из знака приложения */
    --accent: #5b5bd6;
    --accent-fg: #ffffff;
    --accent-text: #4a4ac2;
    /* Состояния */
    --danger: #d93036;
    --ok: #1f9d55;
    /* Нужно внимание: находки LLM ждут проверки. */
    --warn: #c46a0a;
    --warn-fg: #ffffff;
    /* Размеры */
    --r-sm: 6px;
    --r-md: 8px;
    --r-lg: 12px;
    --r-xl: 16px;
    --radius: var(--r-md);
    --fs-xs: 11px;
    --fs-sm: 12px;
    --fs-md: 13px;
    --fs-lg: 15px;
    --fs-xl: 20px;
    /* Тени и движение */
    --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.12);
    --shadow-md: 0 8px 30px rgba(0, 0, 0, 0.14), 0 0 0 0.5px rgba(0, 0, 0, 0.06);
    --shadow-lg: 0 24px 60px rgba(0, 0, 0, 0.24);
    --shadow: var(--shadow-md);
    --ease: cubic-bezier(0.2, 0.8, 0.2, 1);
    color-scheme: light;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: #1c1c1e;
      --bg-sidebar: #232326;
      --bg-card: #252528;
      --bg-elevated: #2c2c30;
      --bg-input: #2a2a2d;
      --bg-hover: rgba(255, 255, 255, 0.06);
      --bg-active: rgba(255, 255, 255, 0.1);
      --bg-selected: rgba(124, 124, 240, 0.22);
      --scrim: rgba(0, 0, 0, 0.5);
      --fg: #ececf0;
      --fg-muted: #9d9da6;
      --fg-faint: #6c6c75;
      --border: rgba(255, 255, 255, 0.09);
      --border-strong: rgba(255, 255, 255, 0.2);
      --accent: #7c7cf0;
      --accent-text: #a3a3ff;
      --danger: #ff6369;
      --ok: #3dd68c;
      --warn: #f0a13a;
      --warn-fg: #1c1c1e;
      --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.4);
      --shadow-md: 0 8px 30px rgba(0, 0, 0, 0.5), 0 0 0 0.5px rgba(255, 255, 255, 0.08);
      --shadow-lg: 0 24px 60px rgba(0, 0, 0, 0.6);
      color-scheme: dark;
    }
  }

  :global(*) { box-sizing: border-box; }
  :global(html, body) {
    margin: 0; height: 100%; overflow: hidden;
    font: var(--fs-md) / 1.45 -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", system-ui, sans-serif;
    background: var(--bg); color: var(--fg);
    -webkit-font-smoothing: antialiased;
    user-select: none; -webkit-user-select: none; cursor: default;
  }
  :global(h1) { font-size: var(--fs-xl); font-weight: 650; margin: 0; letter-spacing: -0.01em; }
  :global(h2) { font-size: var(--fs-md); font-weight: 600; margin: 0 0 8px; }
  :global(.muted) { color: var(--fg-muted); }
  :global(.faint) { color: var(--fg-faint); }

  /* Поля ввода */
  :global(input, textarea, select, button) { font: inherit; color: inherit; }
  :global(input[type="text"], input[type="password"], input[type="search"], input[type="number"], textarea, select) {
    background: var(--bg-input); border: 1px solid var(--border); border-radius: var(--r-sm);
    padding: 5px 8px; outline: none; user-select: text; -webkit-user-select: text; width: 100%;
  }
  :global(select) { width: auto; padding-right: 4px; }
  :global(input:focus, textarea:focus, select:focus) { border-color: var(--accent); box-shadow: 0 0 0 3px var(--bg-selected); }
  :global(input::placeholder) { color: var(--fg-faint); }

  /* Кнопки: обычная, главная, без рамки, опасная, ссылка */
  :global(button) {
    border: 1px solid var(--border); background: var(--bg-input); border-radius: var(--r-sm);
    padding: 4px 11px; cursor: default; white-space: nowrap;
  }
  :global(button:hover) { background: var(--bg-hover); }
  :global(button:active:not(:disabled)) { background: var(--bg-active); }
  :global(button:disabled) { opacity: 0.5; }
  :global(button:focus-visible, [role="switch"]:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  :global(button.primary) { background: var(--accent); color: var(--accent-fg); border-color: transparent; }
  :global(button.primary:hover, button.primary:active:not(:disabled)) { background: var(--accent); filter: brightness(1.08); }
  :global(button.destructive) { background: var(--danger); color: #fff; border-color: transparent; }
  :global(button.destructive:hover, button.destructive:active:not(:disabled)) { background: var(--danger); filter: brightness(1.08); }
  :global(button.ghost) { border-color: transparent; background: transparent; }
  :global(button.ghost:hover) { background: var(--bg-hover); }
  :global(button.danger) { color: var(--danger); }
  :global(button.confirm) { color: var(--ok); }
  :global(button.link) { border: none; background: none; color: var(--accent-text); padding: 0; border-radius: 3px; }
  :global(button.link:hover) { background: none; text-decoration: underline; }
  :global(button.with-icon) { display: inline-flex; align-items: center; gap: 6px; }

  :global(kbd) {
    font: inherit; font-size: var(--fs-xs); min-width: 20px; padding: 1px 5px; text-align: center; border-radius: 5px;
    background: var(--bg-card); border: 1px solid var(--border); box-shadow: 0 1px 0 var(--border); color: var(--fg-muted);
  }
  :global(code) { background: var(--bg-hover); padding: 0 4px; border-radius: 4px; font-size: 11.5px; user-select: text; -webkit-user-select: text; }

  :global(.spinner) {
    display: inline-block; width: 12px; height: 12px; flex-shrink: 0; border-radius: 50%;
    border: 2px solid var(--border-strong); border-top-color: var(--accent); animation: spin 0.8s linear infinite;
  }
  :global(.spinner.large) { width: 26px; height: 26px; border-width: 3px; }
  @keyframes -global-spin { to { transform: rotate(360deg); } }

  /* Таблицы справочников */
  :global(.grid) { width: 100%; border-collapse: collapse; table-layout: fixed; }
  :global(.grid th) {
    text-align: left; font-size: var(--fs-xs); font-weight: 600; color: var(--fg-faint);
    text-transform: uppercase; letter-spacing: 0.04em; padding: 0 6px 6px;
  }
  :global(.grid td) { padding: 3px; vertical-align: top; }
  :global(.grid td input) { border-color: transparent; background: transparent; }
  :global(.grid tr:hover td input) { border-color: var(--border); background: var(--bg-input); }
  :global(.grid td.tools) { white-space: nowrap; text-align: right; }
  :global(.grid th.tools) { width: 76px; }
  :global(.grid td.tools button) { display: inline-flex; align-items: center; padding: 5px; vertical-align: middle; }
  /* Находки LLM, ждущие проверки: подсвеченные строки и полоса с действиями над таблицей. */
  :global(.grid tr.pending td) { background: color-mix(in srgb, var(--warn) 9%, transparent); }
  :global(.grid tr.pending td:first-child) { box-shadow: inset 3px 0 0 var(--warn); }
  :global(.review) {
    display: flex; align-items: center; gap: 8px; padding: 8px 10px 8px 14px; margin-bottom: 12px; border-radius: var(--r-md);
    background: color-mix(in srgb, var(--warn) 12%, transparent); box-shadow: inset 3px 0 0 var(--warn);
  }
  :global(.review .text) { flex: 1; }
  :global(.empty-state) {
    display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 48px 0; text-align: center;
    color: var(--fg-muted);
  }
  :global(.empty-state p) { margin: 0; max-width: 460px; }
  :global(.empty-state svg) { color: var(--fg-faint); margin-bottom: 4px; }

  :global(::-webkit-scrollbar) { width: 10px; height: 10px; }
  :global(::-webkit-scrollbar-thumb) { background: var(--border-strong); border-radius: 10px; border: 3px solid transparent; background-clip: padding-box; }
  @media (prefers-reduced-motion: reduce) {
    :global(.spinner) { animation-duration: 2.4s; }
  }
</style>
