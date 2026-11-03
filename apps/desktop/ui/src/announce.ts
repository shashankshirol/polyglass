// REN-06: new translations are announced to the screen reader through a polite live region, at
// most once every 2 s, each block with its own `lang` so the reader picks a Swedish or English
// voice. Pure logic (the clock is passed in), so `npm test` covers it.

/** One block as the screen reader gets it. */
export interface Spoken {
  text: string;
  lang: string;
}

/** Minimum time between two announcements (REN-06). */
export const ANNOUNCE_GAP_MS = 2000;

export class Announcer {
  private lastAt = Number.NEGATIVE_INFINITY;
  private lastKey = "";
  private pending: Spoken[] | null = null;
  private readonly gapMs: number;

  constructor(gapMs = ANNOUNCE_GAP_MS) {
    this.gapMs = gapMs;
  }

  /**
   * The lens shows `items` at `now` (ms). Returns what to announce right away, or `null`; when
   * something has to wait for the gap, `waitMs` says when to call `flush`. What is already
   * announced (the same texts) isn't repeated, and only the newest of several waiting updates is
   * announced.
   */
  offer(items: readonly Spoken[], now: number): { announce: Spoken[] | null; waitMs: number | null } {
    const key = items.map((i) => `${i.lang}\u0000${i.text}`).join("\u0001");
    if (items.length === 0 || key === this.lastKey) {
      // Nothing new: an older waiting update is stale now.
      this.pending = null;
      return { announce: null, waitMs: null };
    }
    const wait = this.lastAt + this.gapMs - now;
    if (wait <= 0) {
      this.pending = null;
      return { announce: this.take(items, key, now), waitMs: null };
    }
    this.pending = [...items];
    return { announce: null, waitMs: wait };
  }

  /** The gap is over: the waiting update, if it is still the newest. */
  flush(now: number): Spoken[] | null {
    const items = this.pending;
    if (!items || now < this.lastAt + this.gapMs) return null;
    this.pending = null;
    return this.take(items, items.map((i) => `${i.lang}\u0000${i.text}`).join("\u0001"), now);
  }

  /** Forget what was said (pause, or the lens was hidden), so the next text is announced. */
  reset(): void {
    this.lastKey = "";
    this.pending = null;
  }

  private take(items: readonly Spoken[], key: string, now: number): Spoken[] {
    this.lastAt = now;
    this.lastKey = key;
    return [...items];
  }
}
