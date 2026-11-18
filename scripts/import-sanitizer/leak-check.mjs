// Leak check: the sanitized output must not contain any input string (or distinctive token)
// longer than a small threshold, except values an allow-list rule deliberately preserved.
//
// Input strings are held only as 64-bit hashes in a bounded in-memory table; the check never
// writes, logs or returns any input content, only counts.

import { BudgetError, DEFAULT_BUDGETS } from './budgets.mjs';

export const LEAK_MIN_CHARS = 8;
const TOKEN_SPLIT = /[^\p{L}\p{N}_-]+/u;

function hash64(text) {
  let a = 0x811c9dc5;
  let b = 0x9747b28c;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    a = Math.imul(a ^ c, 0x01000193) >>> 0;
    b = Math.imul(b ^ (c + 0x9e37), 0x85ebca6b) >>> 0;
    b = (b ^ (b >>> 13)) >>> 0;
  }
  return [a, (b | 1) >>> 0]; // low word is never zero, so zero marks an empty slot
}

class HashSet64 {
  constructor(capacity = 1 << 16) {
    this.capacity = capacity;
    this.hi = new Uint32Array(capacity);
    this.lo = new Uint32Array(capacity);
    this.size = 0;
  }

  #slot(hi, lo, hiArr, loArr, mask) {
    let i = (hi ^ Math.imul(lo, 0x9e3779b1)) & mask;
    while (loArr[i] !== 0 && !(hiArr[i] === hi && loArr[i] === lo)) i = (i + 1) & mask;
    return i;
  }

  has(hi, lo) {
    const i = this.#slot(hi, lo, this.hi, this.lo, this.capacity - 1);
    return this.lo[i] !== 0;
  }

  add(hi, lo) {
    if ((this.size + 1) * 2 > this.capacity) this.#grow();
    const i = this.#slot(hi, lo, this.hi, this.lo, this.capacity - 1);
    if (this.lo[i] === 0) {
      this.hi[i] = hi;
      this.lo[i] = lo;
      this.size++;
      return true;
    }
    return false;
  }

  #grow() {
    const capacity = this.capacity * 2;
    const hi = new Uint32Array(capacity);
    const lo = new Uint32Array(capacity);
    for (let i = 0; i < this.capacity; i++) {
      if (this.lo[i] === 0) continue;
      const slot = this.#slot(this.hi[i], this.lo[i], hi, lo, capacity - 1);
      hi[slot] = this.hi[i];
      lo[slot] = this.lo[i];
    }
    this.capacity = capacity;
    this.hi = hi;
    this.lo = lo;
  }
}

/** Counts distinct strings by 64-bit hash (bounded memory, no strings retained). */
export class DistinctCounter {
  constructor({ max, budget }) {
    this.max = max;
    this.budget = budget;
    this.set = new HashSet64(1 << 8);
  }
  add(text) {
    const [hi, lo] = hash64(text);
    if (this.set.size >= this.max && !this.set.has(hi, lo)) throw new BudgetError(this.budget, this.max);
    this.set.add(hi, lo);
  }
  get size() { return this.set.size; }
}

export class LeakError extends Error {
  constructor(count) {
    super(`E_LEAK ${count} input string(s) of at least ${LEAK_MIN_CHARS} characters survived in the output`);
    this.code = 'E_LEAK';
    this.count = count;
  }
}

export class LeakRegistry {
  constructor({ maxHashes = DEFAULT_BUDGETS.maxProtectedPieces, maxKept = DEFAULT_BUDGETS.maxKeptPieces } = {}) {
    this.maxHashes = maxHashes;
    this.maxKept = maxKept;
    this.protectedSet = new HashSet64();
    this.keptSet = new HashSet64();
    this.partial = false;
    this.protectedStrings = 0;
  }

  #add(set, text) {
    const [hi, lo] = hash64(text);
    if (set === this.protectedSet && set.size >= this.maxHashes && !set.has(hi, lo)) {
      this.partial = true; // the run stops at the next member boundary (E_LEAK_COVERAGE)
      return;
    }
    if (set === this.keptSet && set.size >= this.maxKept && !set.has(hi, lo)) {
      throw new BudgetError('maxKeptPieces', this.maxKept);
    }
    set.add(hi, lo);
  }

  #each(text, fn) {
    if (text.length >= LEAK_MIN_CHARS) fn(text.trim());
    for (const token of text.split(TOKEN_SPLIT)) if (token.length >= LEAK_MIN_CHARS) fn(token);
  }

  /** Registers an input string that must not appear in the output. */
  protect(text) {
    if (typeof text !== 'string' || text.length < LEAK_MIN_CHARS) return;
    this.protectedStrings++;
    this.#each(text, piece => { if (piece.length >= LEAK_MIN_CHARS) this.#add(this.protectedSet, piece); });
  }

  /** Registers a string an allow-list rule intentionally preserves in the output. */
  markKept(text) {
    if (typeof text !== 'string' || text.length < LEAK_MIN_CHARS) return;
    this.#each(text, piece => { if (piece.length >= LEAK_MIN_CHARS) this.#add(this.keptSet, piece); });
  }

  #leaks(piece) {
    const [hi, lo] = hash64(piece);
    return this.protectedSet.has(hi, lo) && !this.keptSet.has(hi, lo);
  }

  /** Number of leaking pieces (whole string or distinctive tokens) in one output string. */
  countLeaks(text) {
    if (text.length < LEAK_MIN_CHARS) return 0;
    let leaks = 0;
    this.#each(text, piece => { if (piece.length >= LEAK_MIN_CHARS && this.#leaks(piece)) leaks++; });
    return leaks;
  }

  summary() {
    return {
      protectedStrings: this.protectedStrings,
      distinctProtectedPieces: this.protectedSet.size,
      coverage: this.partial ? 'partial' : 'full',
      minChars: LEAK_MIN_CHARS,
    };
  }
}
