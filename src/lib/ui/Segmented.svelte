<script lang="ts" generics="T extends string | number">
  import Icon from "../Icon.svelte";

  /** Переключатель из нескольких вариантов. */
  let { value = $bindable(), options, small = false, onchange }: {
    value: T;
    options: { value: T; label: string; icon?: string; title?: string }[];
    small?: boolean;
    onchange?: (v: T) => void;
  } = $props();
</script>

<div class="seg" class:small role="radiogroup">
  {#each options as o (o.value)}
    <button type="button" role="radio" aria-checked={value === o.value} class:on={value === o.value} title={o.title}
      onclick={() => { if (value !== o.value) { value = o.value; onchange?.(o.value); } }}>
      {#if o.icon}<Icon name={o.icon} size={13} />{/if}{o.label}
    </button>
  {/each}
</div>

<style>
  .seg { display: inline-flex; background: var(--bg-hover); border-radius: var(--r-md); padding: 2px; gap: 1px; }
  button {
    display: inline-flex; align-items: center; gap: 6px; border: none; background: transparent;
    padding: 3px 12px; border-radius: var(--r-sm); color: var(--fg-muted);
  }
  button:hover { background: transparent; color: var(--fg); }
  button.on { background: var(--bg-elevated); color: var(--fg); box-shadow: var(--shadow-sm); }
  .small button { font-size: var(--fs-sm); padding: 2px 9px; }
</style>
