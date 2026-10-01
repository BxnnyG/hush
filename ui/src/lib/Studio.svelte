<script lang="ts">
  import Card from './Card.svelte';
  import Segmented from './Segmented.svelte';
  import Slider from './Slider.svelte';
  import Toggle from './Toggle.svelte';
  import { store } from './store.svelte';
  import { t } from './i18n';
  import type { Preset, StudioParams } from './types';

  let s = $derived(store.state!);
  let preset = $derived(s.settings.studio.preset);
  let open = $state(false);

  // The advanced sliders show the values of the active preset, and edit a "custom" copy of them.
  let params: StudioParams | null = $derived(
    preset === 'custom' ? s.settings.studio.custom : preset === 'off' ? null : (s.presets[preset] ?? null),
  );

  // "Custom" only shows up once the user has edited a slider.
  let shown = $derived<{ value: Preset; label: string }[]>([
    { value: 'off', label: t('preset.off') },
    { value: 'natural', label: t('preset.natural') },
    { value: 'clear', label: t('preset.clear') },
    { value: 'warm', label: t('preset.warm') },
    ...(preset === 'custom' ? [{ value: 'custom' as Preset, label: t('preset.custom') }] : []),
  ]);

  function pick(p: Preset) {
    store.apply({ studio: { preset: p } });
  }

  function edit(patch: Partial<StudioParams>) {
    const base = params ?? s.presets['natural'];
    store.apply({ studio: { preset: 'custom', custom: { ...base, ...patch } } });
  }

  const db = (v: number) => `${v > 0 ? '+' : ''}${v.toFixed(1)} dB`;
  const pct = (v: number) => (v <= 0 ? t('adv.off') : `${Math.round(v * 100)} %`);
</script>

<Card title={t('studio.title')} sub={t('studio.sub')}>
  <Segmented label={t('studio.title')} options={shown} value={preset} onchange={pick} />

  <button class="adv" aria-expanded={open} onclick={() => (open = !open)} disabled={preset === 'off'}>
    <span class="chev" class:open>›</span>{t('studio.advanced')}
  </button>

  {#if open && params}
    <div class="grid">
      <Slider label={t('adv.lowcut')} min={0} max={300} step={5} value={params.lowcut_hz} format={(v) => (v <= 0 ? t('adv.off') : `${Math.round(v)} Hz`)} onchange={(v) => edit({ lowcut_hz: v })} />
      <Slider label={t('adv.warmth')} min={-6} max={6} step={0.5} value={params.warmth_db} format={db} onchange={(v) => edit({ warmth_db: v })} />
      <Slider label={t('adv.presence')} min={-6} max={6} step={0.5} value={params.presence_db} format={db} onchange={(v) => edit({ presence_db: v })} />
      <Slider label={t('adv.air')} min={-6} max={6} step={0.5} value={params.air_db} format={db} onchange={(v) => edit({ air_db: v })} />
      <Slider label={t('adv.deess')} value={params.deess} format={pct} onchange={(v) => edit({ deess: v })} />
      <Slider label={t('adv.compression')} value={params.compression} format={pct} onchange={(v) => edit({ compression: v })} />
      <Slider label={t('adv.gate')} value={params.gate} format={pct} onchange={(v) => edit({ gate: v })} />
      <div class="line">
        <span>{t('adv.limiter')}</span>
        <Toggle label={t('adv.limiter')} checked={params.limiter} onchange={(v) => edit({ limiter: v })} />
      </div>
    </div>
  {/if}
</Card>

<style>
  .adv {
    margin-top: 12px;
    background: none;
    border: 0;
    padding: 4px 0;
    color: var(--accent);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .adv:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .chev {
    display: inline-block;
    transition: transform 0.2s;
    font-size: 18px;
    line-height: 1;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .grid {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-top: 10px;
  }
  .line {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 13.5px;
  }
</style>
