import { api, onDaemon, onLevels, onState } from './api';
import type { SordinoState, Levels, Patch, Settings } from './types';

/** Reactive app state shared by all components (Svelte 5 runes). */
class Store {
  state = $state<SordinoState | null>(null);
  levels = $state<Levels>({ input_db: -100, output_db: -100 });
  daemonUp = $state(true);
  busy = $state(false);

  async init() {
    await onState((s) => (this.state = s));
    await onLevels((l) => (this.levels = l));
    await onDaemon((up) => {
      this.daemonUp = up;
      if (up) this.refresh();
    });
    await this.refresh();
    api.setWatching(true).catch(() => {});
    // Self-monitoring switches itself off in the daemon unless renewed: it must never get stuck on.
    setInterval(() => {
      if (this.state?.monitoring) api.setMonitor(true).catch(() => {});
    }, 2000);
    document.addEventListener('visibilitychange', () => {
      const visible = document.visibilityState === 'visible';
      api.setWatching(visible).catch(() => {});
      if (!visible && this.state?.monitoring) api.setMonitor(false).catch(() => {});
    });
  }

  async refresh() {
    try {
      this.state = await api.getState();
      this.daemonUp = true;
    } catch {
      this.daemonUp = false;
    }
  }

  /** Apply a settings patch optimistically; the daemon's state event confirms or corrects it. */
  async apply(patch: Patch<Settings>) {
    try {
      await api.apply(patch);
    } catch (e) {
      console.warn('apply failed', e);
      await this.refresh();
    }
  }
}

export const store = new Store();
