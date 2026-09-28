<script lang="ts">
  import { tick } from "svelte";
  import Icon from "../Icon.svelte";
  import { menu, type MenuEntry, type MenuItem } from "./menu.svelte";

  /** Открытые уровни: корень и вложенные меню. */
  type Level = { items: MenuItem[]; x: number; y: number; active: number };
  let levels = $state<Level[]>([]);
  let panels = $state<HTMLElement[]>([]);
  let hoverTimer: ReturnType<typeof setTimeout> | undefined;

  const isEntry = (it: MenuItem): it is MenuEntry => "label" in it;
  const selectable = (it: MenuItem) => isEntry(it) && !it.disabled;

  $effect(() => {
    const cur = menu.current;
    levels = cur ? [{ items: cur.items, x: cur.x, y: cur.y, active: -1 }] : [];
    if (cur) fit(0);
  });

  /** Не даём меню уйти за край окна. */
  async function fit(i: number, anchor?: DOMRect) {
    await tick();
    const el = panels[i];
    const lv = levels[i];
    if (!el || !lv) return;
    const { width, height } = el.getBoundingClientRect();
    const pad = 8;
    if (anchor) {
      lv.x = anchor.right + width + pad > innerWidth ? anchor.left - width + 2 : anchor.right - 2;
      lv.y = anchor.top - 5;
    }
    lv.x = Math.max(pad, Math.min(lv.x, innerWidth - width - pad));
    lv.y = Math.max(pad, Math.min(lv.y, innerHeight - height - pad));
  }

  function openSub(level: number, index: number, row: HTMLElement) {
    const it = levels[level]?.items[index];
    if (!it || !isEntry(it) || !it.items?.length || it.disabled) return;
    levels = [...levels.slice(0, level + 1), { items: it.items, x: 0, y: 0, active: -1 }];
    fit(level + 1, row.getBoundingClientRect());
  }

  function hover(level: number, index: number, row: HTMLElement) {
    levels[level].active = index;
    clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => {
      const it = levels[level]?.items[index];
      if (it && isEntry(it) && it.items?.length) openSub(level, index, row);
      else if (levels.length > level + 1) levels = levels.slice(0, level + 1);
    }, 120);
  }

  function run(it: MenuEntry, level: number, index: number, row: HTMLElement) {
    if (it.disabled) return;
    if (it.items?.length) return openSub(level, index, row);
    menu.close();
    it.action?.();
  }

  function move(step: number) {
    const lv = levels[levels.length - 1];
    const n = lv.items.length;
    let i = lv.active;
    for (let k = 0; k < n; k++) {
      i = (i + step + n) % n;
      if (selectable(lv.items[i])) break;
    }
    lv.active = i;
  }

  function onkeydown(e: KeyboardEvent) {
    if (!levels.length) return;
    const depth = levels.length - 1;
    const lv = levels[depth];
    const row = () => panels[depth]?.querySelector<HTMLElement>(`[data-i="${lv.active}"]`);
    if (e.key === "Escape") menu.close();
    else if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "ArrowLeft" && depth > 0) levels = levels.slice(0, -1);
    else if (e.key === "ArrowRight" || e.key === "Enter" || e.key === " ") {
      const it = lv.items[lv.active];
      const el = row();
      if (!it || !isEntry(it) || !el) return;
      if (it.items?.length) {
        openSub(depth, lv.active, el);
        tick().then(() => move(1));
      } else if (e.key !== "ArrowRight") run(it, depth, lv.active, el);
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<svelte:window
  onkeydowncapture={onkeydown}
  onresize={menu.close}
  onblur={menu.close}
  onwheel={(e) => { if (levels.length && !(e.target as HTMLElement).closest?.(".ctx")) menu.close(); }}
/>

{#if levels.length}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="ctx-backdrop" onmousedown={(e) => { e.preventDefault(); menu.close(); }} oncontextmenu={(e) => { e.preventDefault(); menu.close(); }}></div>
  {#each levels as lv, level (level)}
    <!-- Нажатие не должно уводить фокус из поля ввода: иначе «Вставить» некуда. -->
    <div class="ctx" role="menu" tabindex="-1" bind:this={panels[level]} style="left:{lv.x}px; top:{lv.y}px"
      onmousedown={(e) => e.preventDefault()} oncontextmenu={(e) => e.preventDefault()}>
      {#each lv.items as it, i}
        {#if "separator" in it}
          <div class="sep" role="separator"></div>
        {:else if "heading" in it}
          <div class="heading">{it.heading}</div>
        {:else}
          <button role="menuitem" data-i={i} class:active={lv.active === i} class:danger={it.danger} disabled={it.disabled}
            onmouseenter={(e) => hover(level, i, e.currentTarget)}
            onclick={(e) => run(it, level, i, e.currentTarget)}>
            <span class="ico">
              {#if it.checked}<Icon name="check" size={13} />{:else if it.icon}<Icon name={it.icon} size={14} />{/if}
            </span>
            <span class="label">{it.label}</span>
            {#if it.hint}<span class="hint">{it.hint}</span>{/if}
            {#if it.items?.length}<span class="arrow"><Icon name="chevron-right" size={12} /></span>{/if}
          </button>
        {/if}
      {/each}
    </div>
  {/each}
{/if}

<style>
  .ctx-backdrop { position: fixed; inset: 0; z-index: 900; }
  .ctx {
    position: fixed; z-index: 901; min-width: 200px; max-width: 320px; max-height: calc(100vh - 16px); overflow-y: auto;
    padding: 5px; border-radius: var(--r-lg); background: var(--bg-elevated); border: 1px solid var(--border);
    box-shadow: var(--shadow-md); outline: none;
    animation: pop 0.12s var(--ease);
    -webkit-backdrop-filter: blur(24px) saturate(1.6); backdrop-filter: blur(24px) saturate(1.6);
  }
  @keyframes pop { from { opacity: 0; transform: scale(0.97); } }
  button {
    display: flex; align-items: center; gap: 8px; width: 100%; border: none; background: transparent;
    padding: 5px 8px; border-radius: var(--r-sm); text-align: left; color: var(--fg);
  }
  button:hover { background: transparent; }
  button.active:not(:disabled) { background: var(--accent); color: var(--accent-fg); }
  button.active:not(:disabled) .hint { color: inherit; opacity: 0.8; }
  button.danger:not(.active) { color: var(--danger); }
  button.danger.active:not(:disabled) { background: var(--danger); }
  .ico { width: 16px; display: grid; place-items: center; flex-shrink: 0; }
  .label { flex: 1; overflow: hidden; text-overflow: ellipsis; }
  .hint { color: var(--fg-faint); font-size: var(--fs-sm); }
  .arrow { display: grid; opacity: 0.6; margin-right: -3px; }
  .sep { height: 1px; background: var(--border); margin: 5px 8px; }
  .heading { padding: 5px 8px 3px; font-size: var(--fs-xs); font-weight: 600; color: var(--fg-faint); }
  @media (prefers-reduced-motion: reduce) { .ctx { animation: none; } }
</style>
