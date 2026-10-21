// RAF timestamps can precede performance.now() captured by a resume click.
// Never turn a negative delta into a negative tick count (a WASM u32 wraps it).
const STEP = 1000 / 24;
export function advanceClock(elapsed, delta, busy = false) {
  const total = Math.max(0, Math.min(elapsed + Math.max(0, delta), 4 * STEP));
  const ticks = busy ? 0 : Math.floor(total / STEP);
  return { ticks, elapsed: Math.max(0, total - ticks * STEP) };
}
