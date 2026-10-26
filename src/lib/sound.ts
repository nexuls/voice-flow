let ctx: AudioContext | null = null;

/** Plays a short, quiet sine tone. Used for optional start/stop feedback. */
export function playTone(frequency: number, durationMs = 70, volume = 0.08) {
  try {
    ctx ??= new AudioContext();
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    const now = ctx.currentTime;
    osc.frequency.value = frequency;
    gain.gain.setValueAtTime(0, now);
    gain.gain.linearRampToValueAtTime(volume, now + 0.008);
    gain.gain.exponentialRampToValueAtTime(0.0001, now + durationMs / 1000);
    osc.connect(gain).connect(ctx.destination);
    osc.start(now);
    osc.stop(now + durationMs / 1000 + 0.02);
  } catch {
    // Audio output unavailable: feedback is best-effort.
  }
}

export const startTone = () => playTone(880);
export const stopTone = () => playTone(587);
