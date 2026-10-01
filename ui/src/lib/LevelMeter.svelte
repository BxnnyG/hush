<script lang="ts">
  import { levelToFraction } from './format';
  let { db, label, active = true }: { db: number; label: string; active?: boolean } = $props();
  let frac = $derived(active ? levelToFraction(db) : 0);
  // Peak-hold marker that falls slowly.
  let hold = $state(0);
  $effect(() => {
    if (frac >= hold) hold = frac;
  });
  $effect(() => {
    const id = setInterval(() => (hold = Math.max(frac, hold - 0.03)), 60);
    return () => clearInterval(id);
  });
</script>

<div class="meter" role="meter" aria-label={label} aria-valuemin="-60" aria-valuemax="0" aria-valuenow={Math.round(db)}>
  <span class="lab">{label}</span>
  <div class="track">
    <div class="fill" class:hot={frac > 0.92} style="width:{frac * 100}%"></div>
    <div class="peak" style="left:{hold * 100}%"></div>
  </div>
</div>

<style>
  .meter {
    display: grid;
    grid-template-columns: 150px 1fr;
    align-items: center;
    gap: 10px;
    font-size: 12.5px;
    color: var(--sub);
  }
  .track {
    position: relative;
    height: 8px;
    border-radius: 4px;
    background: var(--track);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 4px;
    background: linear-gradient(90deg, var(--ok), #8ccf3d 70%, #f5b301);
    transition: width 0.07s linear;
  }
  .fill.hot {
    background: var(--bad);
  }
  .peak {
    position: absolute;
    top: 0;
    width: 2px;
    height: 100%;
    background: var(--text);
    opacity: 0.55;
  }
</style>
