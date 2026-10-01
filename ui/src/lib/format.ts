/** Map a dBFS level to 0..1 for display (-60 dB floor). */
export function levelToFraction(db: number): number {
  return Math.min(1, Math.max(0, (db + 60) / 60));
}
