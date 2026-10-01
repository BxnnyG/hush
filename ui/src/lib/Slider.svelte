<script lang="ts">
  let {
    value,
    min = 0,
    max = 1,
    step = 0.01,
    label,
    format,
    onchange,
    ticks,
  }: {
    value: number;
    min?: number;
    max?: number;
    step?: number;
    label: string;
    format?: (v: number) => string;
    onchange: (v: number) => void;
    ticks?: string[];
  } = $props();
  let pct = $derived(((value - min) / (max - min)) * 100);
</script>

<div class="row">
  <div class="top">
    <span>{label}</span>
    {#if format}<span class="val">{format(value)}</span>{/if}
  </div>
  <input type="range" {min} {max} {step} {value} aria-label={label} style="--pct:{pct}%" oninput={(e) => onchange(+(e.currentTarget as HTMLInputElement).value)} />
  {#if ticks}
    <div class="ticks">{#each ticks as tk}<span>{tk}</span>{/each}</div>
  {/if}
</div>

<style>
  .row {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .top {
    display: flex;
    justify-content: space-between;
    font-size: 13.5px;
  }
  .val {
    color: var(--sub);
    font-variant-numeric: tabular-nums;
  }
  input[type='range'] {
    -webkit-appearance: none;
    appearance: none;
    width: 100%;
    height: 28px;
    margin: 0;
    background: transparent;
    cursor: pointer;
  }
  input[type='range']::-webkit-slider-runnable-track {
    height: 6px;
    border-radius: 3px;
    background: linear-gradient(to right, var(--accent) var(--pct), var(--track) var(--pct));
  }
  input[type='range']::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 24px;
    height: 24px;
    margin-top: -9px;
    border-radius: 50%;
    background: #fff;
    border: 0;
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.35);
  }
  .ticks {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    color: var(--sub);
    margin-top: -4px;
  }
</style>
