<script lang="ts">
  /** Выключатель. `label` — для экранного диктора, когда подпись стоит отдельно. */
  let { checked = $bindable(false), disabled = false, label = "", onchange }: {
    checked?: boolean; disabled?: boolean; label?: string; onchange?: (v: boolean) => void;
  } = $props();
</script>

<button type="button" class="toggle" class:on={checked} role="switch" aria-checked={checked} aria-label={label || undefined} {disabled}
  onclick={() => { checked = !checked; onchange?.(checked); }}>
  <span class="knob"></span>
</button>

<style>
  .toggle {
    width: 34px; height: 20px; padding: 2px; border-radius: 10px; border: none; flex-shrink: 0;
    background: var(--border-strong); transition: background 0.15s;
  }
  .toggle:hover { background: var(--border-strong); filter: brightness(0.95); }
  .toggle.on, .toggle.on:hover { background: var(--accent); }
  .knob {
    display: block; width: 16px; height: 16px; border-radius: 50%; background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25); transition: transform 0.15s var(--ease);
  }
  .on .knob { transform: translateX(14px); }
  @media (prefers-reduced-motion: reduce) { .toggle, .knob { transition: none; } }
</style>
