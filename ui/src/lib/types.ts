// Mirrors crates/sordino-core/src/ipc.rs, settings.rs and studio.rs.

export type Status = 'off' | 'starting' | 'running' | 'mic_missing' | 'no_pipewire' | 'error';
export type Strength = 'light' | 'medium' | 'high' | 'max';
export type Preset = 'off' | 'natural' | 'clear' | 'warm' | 'custom';
export type ProfileKind = 'voice' | 'studio' | 'stereo' | 'headset' | 'music' | 'off' | 'other';
export type DeviceKind =
  | 'usb'
  | 'builtin'
  | 'bluetooth'
  | 'headset'
  | 'headphones'
  | 'speaker'
  | 'hdmi'
  | 'webcam'
  | 'loopback'
  | 'other';

export interface StudioParams {
  lowcut_hz: number;
  warmth_db: number;
  presence_db: number;
  air_db: number;
  deess: number;
  compression: number;
  gate: number;
  limiter: boolean;
}

export interface Settings {
  enabled: boolean;
  mic: string | null;
  set_default: boolean;
  run_in_background: boolean;
  show_all_devices: boolean;
  noise: { enabled: boolean; strength: Strength };
  echo: { enabled: boolean };
  studio: { preset: Preset; custom: StudioParams };
}

export interface ProfileInfo {
  index: number;
  name: string;
  description: string;
  kind: ProfileKind;
  available: boolean;
  priority: number;
}

export interface Device {
  id: string;
  node_id: number;
  name: string;
  kind: DeviceKind;
  card: number | null;
  profile: ProfileInfo | null;
}

export interface ProfileHint {
  card: number;
  device_id: string;
  device_name: string;
  current: ProfileInfo;
  suggested: ProfileInfo;
}

export interface SordinoState {
  version: string;
  status: Status;
  error: string | null;
  settings: Settings;
  devices: Device[];
  hidden_devices: Device[];
  active_mic: string | null;
  sinks: Device[];
  default_source: string | null;
  default_sink: string | null;
  diag: Diag;
  profile_hint: ProfileHint | null;
  latency_ms: number | null;
  default_is_sordino: boolean;
  monitoring: boolean;
  ab_original: boolean;
  echo_available: boolean;
  presets: Record<string, StudioParams>;
}

export interface Diag {
  out_underruns: number;
  out_skipped: number;
  in_dropped: number;
  model_errors: number;
  quantum: number;
  out_callbacks: number;
  dsp_priority: number;
  overload_hops: number;
  overload_events: number;
}

export interface AutostartStatus {
  app: boolean;
  daemon: boolean;
  flatpak: boolean;
}

export interface Levels {
  input_db: number;
  output_db: number;
}

/** Deep partial used for settings patches. */
export type Patch<T> = { [K in keyof T]?: T[K] extends object ? Patch<T[K]> : T[K] };
