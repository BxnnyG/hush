import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AutostartStatus, SordinoState, Levels, Patch, Settings } from './types';

/** Thin wrapper over the Tauri commands, which in turn talk to sordinod over D-Bus. */
export const api = {
  getState: () => invoke<SordinoState>('get_state'),
  apply: (patch: Patch<Settings>) => invoke<void>('apply', { patch: JSON.stringify(patch) }),
  setProfile: (card: number, index: number) => invoke<void>('set_profile', { card, index }),
  setMonitor: (on: boolean) => invoke<void>('set_monitor', { on }),
  setAbOriginal: (on: boolean) => invoke<void>('set_ab_original', { on }),
  setWatching: (on: boolean) => invoke<void>('set_watching', { on }),
  getAutostart: () => invoke<AutostartStatus>('get_autostart'),
  setAutostart: (on: boolean) => invoke<void>('set_autostart', { on }),
  setDaemonAutostart: (on: boolean) => invoke<void>('set_daemon_autostart', { on }),
  setDefaultDevice: (kind: 'source' | 'sink', name: string) => invoke<void>('set_default_device', { kind, name }),
  startDaemon: () => invoke<void>('start_daemon'),
  quit: () => invoke<void>('quit_app'),
  openRepo: () => invoke<void>('open_repo'),
};

export function onState(cb: (s: SordinoState) => void) {
  return listen<SordinoState>('sordino://state', (e) => cb(e.payload));
}
export function onLevels(cb: (l: Levels) => void) {
  return listen<Levels>('sordino://levels', (e) => cb(e.payload));
}
export function onDaemon(cb: (up: boolean) => void) {
  return listen<boolean>('sordino://daemon', (e) => cb(e.payload));
}
