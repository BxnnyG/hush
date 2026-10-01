<script lang="ts">
  import { onMount } from 'svelte';
  import Card from './lib/Card.svelte';
  import LevelMeter from './lib/LevelMeter.svelte';
  import Segmented from './lib/Segmented.svelte';
  import Settings from './lib/Settings.svelte';
  import Slider from './lib/Slider.svelte';
  import Studio from './lib/Studio.svelte';
  import Toggle from './lib/Toggle.svelte';
  import { api } from './lib/api';
  import { t } from './lib/i18n';
  import { store } from './lib/store.svelte';
  import type { Strength } from './lib/types';

  let showSettings = $state(false);
  let fixing = $state(false);
  let copied = $state(false);
  let showError = $state(false);

  onMount(() => {
    store.init();
  });

  let s = $derived(store.state);
  let status = $derived(!store.daemonUp ? 'no_daemon' : (s?.status ?? 'starting'));
  let running = $derived(status === 'running');
  let devices = $derived(s ? (s.settings.show_all_devices ? [...s.devices, ...s.hidden_devices] : s.devices) : []);
  let micValue = $derived(s?.settings.mic ?? '');

  const strengths: Strength[] = ['light', 'medium', 'high', 'max'];
  let strengthIndex = $derived(s ? strengths.indexOf(s.settings.noise.strength) : 2);

  async function fixProfile() {
    if (!s?.profile_hint) return;
    fixing = true;
    try {
      await api.setProfile(s.profile_hint.card, s.profile_hint.suggested.index);
    } finally {
      setTimeout(() => (fixing = false), 1500);
    }
  }

  async function copyName() {
    try {
      await navigator.clipboard.writeText('Sordino Mic');
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      /* clipboard may be unavailable; the name is shown anyway */
    }
  }

  let ab = $state(false);
  function abDown() {
    ab = true;
    api.setAbOriginal(true);
  }
  function abUp() {
    if (!ab) return;
    ab = false;
    api.setAbOriginal(false);
  }
</script>

<svelte:window onblur={abUp} onpointerup={abUp} />

<main>
  <header class="top">
    <h1>{t('app.title')}</h1>
    <div class="pill {status}">
      <span class="dot"></span>{t(`status.${status}`)}
    </div>
    <button class="gear" aria-label={t('settings.title')} onclick={() => (showSettings = true)}>
      <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" /></svg>
    </button>
  </header>

  {#if !store.daemonUp}
    <Card title={t('status.no_daemon')} sub={t('error.nodaemon')}>
      <button class="primary" onclick={() => api.startDaemon().then(() => store.refresh())}>{t('error.start')}</button>
    </Card>
  {:else if s}
    {#if status === 'no_pipewire'}
      <div class="banner bad"><strong>{t('status.no_pipewire')}</strong><p>{t('error.nopw')}</p></div>
    {:else if status === 'error'}
      <div class="banner bad">
        <strong>{t('status.error')}</strong>
        <p>{t('error.retry')}</p>
        {#if s.error}
          <button class="link" onclick={() => (showError = !showError)}>{t('error.title')}</button>
          {#if showError}<code>{s.error}</code>{/if}
        {/if}
      </div>
    {:else if status === 'mic_missing'}
      <div class="banner warn"><strong>{t('status.mic_missing')}</strong><p>{t('error.micmissing')}</p></div>
    {/if}

    {#if s.profile_hint}
      <div class="banner warn">
        <strong>{t('profile.title')}</strong>
        <p>{t('profile.body', { name: s.profile_hint.device_name })}</p>
        <button class="primary small" disabled={fixing} onclick={fixProfile}>{fixing ? t('profile.fixing') : t('profile.fix')}</button>
      </div>
    {/if}

    <Card title={t('mic.title')}>
      <div class="select">
        <select
          aria-label={t('mic.title')}
          value={micValue}
          onchange={(e) => store.apply({ mic: e.currentTarget.value === '' ? null : e.currentTarget.value })}
        >
          <option value="">{t('mic.auto')}</option>
          {#each devices as d (d.id)}
            <option value={d.id}>{d.name}</option>
          {/each}
          {#if s.settings.mic && !devices.some((d) => d.id === s.settings.mic)}
            <option value={s.settings.mic}>{s.settings.mic}</option>
          {/if}
        </select>
      </div>
      <div class="meters">
        <LevelMeter label={t('meter.mic')} db={store.levels.input_db} active={running} />
        <LevelMeter label={t('meter.out')} db={store.levels.output_db} active={running} />
      </div>
    </Card>

    <Card title={t('noise.title')} sub={t('noise.sub')}>
      {#snippet control()}
        <Toggle label={t('noise.title')} checked={s.settings.noise.enabled} onchange={(v) => store.apply({ noise: { enabled: v } })} />
      {/snippet}
      {#if s.settings.noise.enabled}
        <Slider
          label={t('noise.strength')}
          min={0}
          max={3}
          step={1}
          value={strengthIndex}
          format={(v) => t(`strength.${strengths[v]}`)}
          onchange={(v) => store.apply({ noise: { strength: strengths[v] } })}
          ticks={strengths.map((x) => t(`strength.${x}`))}
        />
      {/if}
    </Card>

    <Card title={t('echo.title')} sub={t('echo.sub')} dim={!s.echo_available}>
      {#snippet control()}
        {#if s.echo_available}
          <Toggle label={t('echo.title')} checked={s.settings.echo.enabled} onchange={(v) => store.apply({ echo: { enabled: v } })} />
        {:else}
          <span class="soon">{t('echo.soon')}</span>
        {/if}
      {/snippet}
    </Card>

    <Studio />

    <Card title={t('test.title')}>
      <div class="test">
        <div class="line">
          <div>
            <span>{t('test.listen')}</span>
            <small>{t('test.listen_sub')}</small>
          </div>
          <Toggle label={t('test.listen')} checked={s.monitoring} disabled={!running} onchange={(v) => api.setMonitor(v)} />
        </div>
        {#if s.monitoring}
          <button class="ab" class:held={s.ab_original} onpointerdown={abDown} onkeydown={(e) => (e.key === ' ' || e.key === 'Enter') && abDown()} onkeyup={abUp}>
            <strong>{s.ab_original ? t('test.ab_original') : t('test.ab_processed')}</strong>
            <small>{t('test.ab')}</small>
          </button>
        {/if}
      </div>
    </Card>

    <footer>
      <div>
        {t('footer.pick')}: <button class="name" onclick={copyName} title={t('footer.copy')}>Sordino Mic</button>
        {#if copied}<span class="copied">{t('footer.copied')}</span>{/if}
      </div>
      {#if s.latency_ms !== null}<div class="lat">{t('footer.latency', { ms: Math.round(s.latency_ms) })}</div>{/if}
    </footer>
  {/if}

  <div class="credit">
    <button onclick={() => api.openRepo()} title="https://github.com/BxnnyG/sordino">{t('footer.credit')} · github.com/BxnnyG/sordino</button>
    <span>{t('footer.license')}</span>
  </div>
</main>

{#if showSettings && s}
  <Settings onclose={() => (showSettings = false)} />
{/if}

<style>
  main {
    max-width: 480px;
    margin: 0 auto;
    padding: 18px 16px 28px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 2px 4px;
  }
  h1 {
    margin: 0;
    font-size: 26px;
    letter-spacing: -0.02em;
    flex: 1;
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-size: 13px;
    color: var(--sub);
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--sub);
  }
  .pill.running .dot {
    background: var(--ok);
    box-shadow: 0 0 0 4px color-mix(in srgb, var(--ok) 25%, transparent);
  }
  .pill.starting .dot,
  .pill.mic_missing .dot {
    background: var(--warn);
  }
  .pill.error .dot,
  .pill.no_pipewire .dot,
  .pill.no_daemon .dot {
    background: var(--bad);
  }
  .gear {
    border: 0;
    background: none;
    color: var(--sub);
    padding: 6px;
    border-radius: 8px;
    cursor: pointer;
    display: grid;
  }
  .gear:hover {
    color: var(--text);
  }
  .select select {
    width: 100%;
    appearance: none;
    -webkit-appearance: none;
    border: 0;
    border-radius: 12px;
    background: var(--track) url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='8' viewBox='0 0 12 8'><path d='M1 1l5 5 5-5' fill='none' stroke='%23888' stroke-width='1.8' stroke-linecap='round'/></svg>") no-repeat right 14px center;
    color: var(--text);
    padding: 12px 38px 12px 14px;
    font: inherit;
    cursor: pointer;
  }
  .meters {
    margin-top: 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .banner {
    border-radius: var(--radius);
    padding: 14px 16px;
    font-size: 14px;
  }
  .banner p {
    margin: 4px 0 0;
    color: var(--sub);
  }
  .banner.warn {
    background: var(--warn-bg);
    border: 1px solid color-mix(in srgb, var(--warn) 35%, transparent);
  }
  .banner.bad {
    background: color-mix(in srgb, var(--bad) 12%, var(--card));
    border: 1px solid color-mix(in srgb, var(--bad) 35%, transparent);
  }
  .banner code {
    display: block;
    margin-top: 8px;
    font-size: 12px;
    user-select: text;
    word-break: break-word;
  }
  .primary {
    margin-top: 10px;
    border: 0;
    border-radius: 12px;
    padding: 11px 18px;
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 600;
    cursor: pointer;
  }
  .primary.small {
    padding: 8px 14px;
    font-size: 14px;
  }
  .primary:disabled {
    opacity: 0.6;
  }
  .link {
    margin-top: 6px;
    border: 0;
    background: none;
    color: var(--accent);
    padding: 0;
    cursor: pointer;
  }
  .soon {
    white-space: nowrap;
    font-size: 12.5px;
    color: var(--sub);
    background: var(--track);
    padding: 4px 10px;
    border-radius: 999px;
  }
  .test {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }
  small {
    display: block;
    color: var(--sub);
    font-size: 12.5px;
  }
  .ab {
    border: 0;
    border-radius: 14px;
    padding: 16px;
    background: var(--track);
    cursor: pointer;
    text-align: center;
    touch-action: none;
  }
  .ab.held {
    background: var(--accent);
    color: var(--accent-text);
  }
  .ab.held small {
    color: inherit;
    opacity: 0.85;
  }
  footer {
    text-align: center;
    color: var(--sub);
    font-size: 13px;
    padding-top: 6px;
  }
  .name {
    border: 0;
    background: none;
    color: var(--text);
    font-weight: 600;
    cursor: pointer;
    padding: 0;
    font-size: inherit;
  }
  .copied {
    margin-left: 6px;
    color: var(--ok);
  }
  .lat {
    margin-top: 2px;
    font-size: 12px;
  }
  .credit {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding-top: 10px;
    font-size: 12px;
    color: var(--sub);
  }
  .credit button {
    border: 0;
    background: none;
    color: var(--sub);
    font-size: inherit;
    cursor: pointer;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .credit button:hover {
    color: var(--text);
  }
</style>
