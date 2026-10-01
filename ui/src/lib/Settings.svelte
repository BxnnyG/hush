<script lang="ts">
  import { onMount } from 'svelte';
  import Toggle from './Toggle.svelte';
  import { api } from './api';
  import { store } from './store.svelte';
  import { t } from './i18n';
  import type { AutostartStatus } from './types';

  let { onclose }: { onclose: () => void } = $props();
  let s = $derived(store.state!);

  let auto = $state<AutostartStatus>({ app: false, daemon: false, flatpak: false });
  let autoError = $state('');

  async function refreshAuto() {
    try {
      auto = await api.getAutostart();
    } catch (e) {
      autoError = String(e);
    }
  }
  onMount(refreshAuto);

  async function setApp(on: boolean) {
    autoError = '';
    try {
      await api.setAutostart(on);
    } catch (e) {
      autoError = String(e);
    }
    await refreshAuto();
  }

  async function setDaemon(on: boolean) {
    autoError = '';
    try {
      await api.setDaemonAutostart(on);
    } catch (e) {
      autoError = String(e);
    }
    await refreshAuto();
  }

  let mics = $derived(s.settings.show_all_devices ? [...s.devices, ...s.hidden_devices] : s.devices);
</script>

<div class="scrim" role="presentation" onclick={onclose}></div>
<div class="sheet" role="dialog" aria-modal="true" aria-label={t('settings.title')}>
  <header>
    <h2>{t('settings.title')}</h2>
    <button class="done" onclick={onclose}>{t('settings.close')}</button>
  </header>

  <h3>{t('settings.defaults')}</h3>
  <p class="hint">{t('settings.default_hint')}</p>
  <label class="pick">
    <span>{t('settings.default_mic')}</span>
    <select value={s.default_source ?? ''} onchange={(e) => api.setDefaultDevice('source', e.currentTarget.value)}>
      <option value="sordino_mic">{t('settings.sordino_mic')}</option>
      {#each mics as d (d.id)}
        <option value={d.id}>{d.name}</option>
      {/each}
      {#if s.default_source && s.default_source !== 'sordino_mic' && !mics.some((d) => d.id === s.default_source)}
        <option value={s.default_source}>{s.default_source}</option>
      {/if}
    </select>
  </label>
  <label class="pick">
    <span>{t('settings.default_out')}</span>
    <select value={s.default_sink ?? ''} onchange={(e) => api.setDefaultDevice('sink', e.currentTarget.value)}>
      {#each s.sinks as d (d.id)}
        <option value={d.id}>{d.name}</option>
      {/each}
      {#if s.default_sink && !s.sinks.some((d) => d.id === s.default_sink)}
        <option value={s.default_sink}>{s.default_sink}</option>
      {/if}
    </select>
  </label>

  <h3>{t('settings.startup')}</h3>
  {#if !auto.flatpak}
    <div class="row">
      <div>
        <span>{t('settings.daemon_autostart')}</span>
        <small>{t('settings.daemon_autostart_sub')}</small>
      </div>
      <Toggle label={t('settings.daemon_autostart')} checked={auto.daemon} onchange={setDaemon} />
    </div>
  {/if}
  <div class="row">
    <div>
      <span>{t('settings.autostart')}</span>
      <small>{auto.flatpak ? t('settings.flatpak_autostart_sub') : t('settings.autostart_sub')}</small>
    </div>
    <Toggle label={t('settings.autostart')} checked={auto.app} onchange={setApp} />
  </div>
  {#if autoError}<p class="err">{autoError}</p>{/if}

  <h3>{t('settings.behaviour')}</h3>
  <div class="row">
    <span>{t('settings.enabled')}</span>
    <Toggle label={t('settings.enabled')} checked={s.settings.enabled} onchange={(v) => store.apply({ enabled: v })} />
  </div>
  <div class="row">
    <span>{t('settings.background')}</span>
    <Toggle label={t('settings.background')} checked={s.settings.run_in_background} onchange={(v) => store.apply({ run_in_background: v })} />
  </div>
  <div class="row">
    <span>{t('mic.show_all')}</span>
    <Toggle label={t('mic.show_all')} checked={s.settings.show_all_devices} onchange={(v) => store.apply({ show_all_devices: v })} />
  </div>

  <button class="quit" onclick={() => api.quit()}>{t('settings.quit')}</button>
  <p class="ver">{t('settings.version', { v: s.version })}</p>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    animation: fade 0.2s;
  }
  .sheet {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    max-height: 92vh;
    overflow: auto;
    background: var(--card);
    border-radius: 22px 22px 0 0;
    padding: 18px 20px 22px;
    box-shadow: 0 -8px 40px rgba(0, 0, 0, 0.25);
    animation: up 0.25s cubic-bezier(0.2, 0.8, 0.3, 1);
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 4px;
  }
  h2 {
    margin: 0;
    font-size: 18px;
  }
  h3 {
    margin: 18px 0 2px;
    font-size: 12.5px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--sub);
  }
  .hint {
    margin: 0 0 8px;
    font-size: 12.5px;
    color: var(--sub);
  }
  .done {
    border: 0;
    background: none;
    color: var(--accent);
    font-weight: 600;
    font-size: 15px;
    cursor: pointer;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 13px 0;
    border-bottom: 1px solid var(--line);
  }
  .pick {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 0;
    font-size: 13.5px;
  }
  .pick select {
    appearance: none;
    -webkit-appearance: none;
    border: 0;
    border-radius: 12px;
    background: var(--track) url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='8' viewBox='0 0 12 8'><path d='M1 1l5 5 5-5' fill='none' stroke='%23888' stroke-width='1.8' stroke-linecap='round'/></svg>") no-repeat right 14px center;
    color: var(--text);
    padding: 11px 38px 11px 14px;
    font: inherit;
    cursor: pointer;
  }
  small {
    display: block;
    color: var(--sub);
    font-size: 12.5px;
    margin-top: 2px;
  }
  .err {
    color: var(--bad);
    font-size: 12.5px;
    margin: 6px 0 0;
  }
  .quit {
    margin-top: 18px;
    width: 100%;
    padding: 12px;
    border-radius: 12px;
    border: 0;
    background: var(--track);
    color: var(--bad);
    font-weight: 600;
    cursor: pointer;
  }
  .ver {
    text-align: center;
    color: var(--sub);
    font-size: 12px;
    margin: 12px 0 0;
  }
  @keyframes up {
    from {
      transform: translateY(40px);
      opacity: 0;
    }
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
