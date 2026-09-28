<script lang="ts">
  let { children } = $props();
</script>

{@render children()}

<style>
  :global(:root) {
    --bg: #ffffff;
    --bg-sidebar: #f3f3f5;
    --bg-hover: rgba(0, 0, 0, 0.05);
    --bg-selected: rgba(91, 91, 214, 0.12);
    --bg-input: #ffffff;
    --bg-card: #f8f8fa;
    --fg: #1c1c1f;
    --fg-muted: #6e6e76;
    --fg-faint: #a0a0a8;
    --border: rgba(0, 0, 0, 0.09);
    --accent: #5b5bd6;
    --accent-fg: #ffffff;
    --danger: #d93036;
    --ok: #1f9d55;
    --radius: 8px;
    --shadow: 0 8px 30px rgba(0, 0, 0, 0.12);
    color-scheme: light;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: #1c1c1e;
      --bg-sidebar: #232326;
      --bg-hover: rgba(255, 255, 255, 0.06);
      --bg-selected: rgba(124, 124, 240, 0.22);
      --bg-input: #2a2a2d;
      --bg-card: #252528;
      --fg: #ececf0;
      --fg-muted: #9a9aa3;
      --fg-faint: #66666e;
      --border: rgba(255, 255, 255, 0.09);
      --accent: #7c7cf0;
      --shadow: 0 8px 30px rgba(0, 0, 0, 0.5);
      color-scheme: dark;
    }
  }
  :global(*) { box-sizing: border-box; }
  :global(html, body) {
    margin: 0; height: 100%; overflow: hidden;
    font: 13px/1.45 -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", system-ui, sans-serif;
    background: var(--bg); color: var(--fg);
    -webkit-font-smoothing: antialiased;
    user-select: none; -webkit-user-select: none; cursor: default;
  }
  :global(input, textarea, select, button) { font: inherit; color: inherit; }
  :global(input[type="text"], input[type="password"], input[type="search"], input[type="number"], textarea, select) {
    background: var(--bg-input); border: 1px solid var(--border); border-radius: 6px;
    padding: 5px 8px; outline: none; user-select: text; -webkit-user-select: text; width: 100%;
  }
  :global(input:focus, textarea:focus, select:focus) { border-color: var(--accent); box-shadow: 0 0 0 3px var(--bg-selected); }
  :global(button) {
    border: 1px solid var(--border); background: var(--bg-input); border-radius: 6px;
    padding: 4px 11px; cursor: default; white-space: nowrap;
  }
  :global(button:hover) { background: var(--bg-hover); }
  :global(button:disabled) { opacity: 0.5; }
  :global(button.primary) { background: var(--accent); color: var(--accent-fg); border-color: transparent; }
  :global(button.primary:hover) { filter: brightness(1.08); }
  :global(button.ghost) { border-color: transparent; background: transparent; }
  :global(button.ghost:hover) { background: var(--bg-hover); }
  :global(button.danger) { color: var(--danger); }
  :global(h1) { font-size: 20px; font-weight: 650; margin: 0; letter-spacing: -0.01em; }
  :global(h2) { font-size: 13px; font-weight: 600; margin: 0 0 8px; }
  :global(.muted) { color: var(--fg-muted); }
  :global(.faint) { color: var(--fg-faint); }
  :global(::-webkit-scrollbar) { width: 10px; height: 10px; }
  :global(::-webkit-scrollbar-thumb) { background: var(--border); border-radius: 10px; border: 3px solid transparent; background-clip: padding-box; }
</style>
