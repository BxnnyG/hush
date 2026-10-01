<script lang="ts" generics="T extends string">
  let { options, value, onchange, label }: { options: { value: T; label: string }[]; value: T; onchange: (v: T) => void; label: string } = $props();
</script>

<div class="seg" role="radiogroup" aria-label={label}>
  {#each options as o (o.value)}
    <button role="radio" aria-checked={value === o.value} class:active={value === o.value} onclick={() => onchange(o.value)}>{o.label}</button>
  {/each}
</div>

<style>
  .seg {
    display: flex;
    gap: 2px;
    padding: 3px;
    background: var(--track);
    border-radius: 11px;
  }
  button {
    flex: 1;
    border: 0;
    background: transparent;
    padding: 7px 4px;
    border-radius: 8px;
    font-size: 13.5px;
    font-weight: 500;
    color: var(--sub);
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
    white-space: nowrap;
  }
  button.active {
    background: var(--card);
    color: var(--text);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.18);
  }
</style>
