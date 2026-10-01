<script lang="ts">
  import Toggle from './Toggle.svelte';
  import { api } from './api';
  import { store } from './store.svelte';
  import { t } from './i18n';

  let { onclose }: { onclose: () => void } = $props();
  let s = $derived(store.state!);

  function setAutostart(on: boolean) {
    // The autostart entry lives in the UI process; the daemon only stores the preference.
    api.setAutostart(on).catch(() => {});
    store.apply({ autostart: on });
  }
</script>

<div class="scrim" role="presentation" onclick={onclose}></div>
<div class="sheet" role="dialog" aria-modal="true" aria-label={t('settings.title')}>
  <header>
    <h2>{t('settings.title')}</h2>
    <button class="done" onclick={onclose}>{t('settings.close')}</button>
  </header>

  <div class="row">
    <span>{t('settings.enabled')}</span>
    <Toggle label={t('settings.enabled')} checked={s.settings.enabled} onchange={(v) => store.apply({ enabled: v })} />
  </div>
  <div class="row">
    <div>
      <span>{t('settings.default')}</span>
      <small>{t('settings.default_sub')}</small>
    </div>
    <Toggle label={t('settings.default')} checked={s.settings.set_default} onchange={(v) => store.apply({ set_default: v })} />
  </div>
  <div class="row">
    <span>{t('settings.autostart')}</span>
    <Toggle label={t('settings.autostart')} checked={s.settings.autostart} onchange={setAutostart} />
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
    max-height: 90vh;
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
    margin-bottom: 8px;
  }
  h2 {
    margin: 0;
    font-size: 18px;
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
  small {
    display: block;
    color: var(--sub);
    font-size: 12.5px;
    margin-top: 2px;
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
