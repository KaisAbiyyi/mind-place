// Explicit resource budgets. Every structure whose size depends on the INPUT (number of distinct
// identifiers, tokens, members, nesting depth, token length) has a named budget here. Exceeding
// one throws BudgetError: the run stops, nothing is published, an existing output is untouched
// and all spool/partial files are removed. Budgets can only be lowered or raised explicitly by the
// caller; the defaults are sized so a real 1 GB export fits with large headroom.

export class BudgetError extends Error {
  constructor(budget, limit) {
    super(`E_BUDGET ${budget} exceeded its limit of ${limit}; the run was stopped and nothing was published`);
    this.code = 'E_BUDGET';
    this.budget = budget;
    this.limit = limit;
    this.exitCode = 5;
  }
}

export const DEFAULT_BUDGETS = Object.freeze({
  maxProtectedPieces: 8_000_000, // leak registry: distinct input strings/tokens of 8+ chars (hash table)
  maxKeptPieces: 1_000_000, // leak registry: distinct allow-listed values
  maxDistinctIdentifiers: 4_000_000, // per JSON member, input and output analysis each
  maxUnknownClassTokens: 1_000_000, // per HTML member
  maxShortIdentifiers: 100_000, // short-ID injectivity table (entries)
  maxShortIdentifierBytes: 8 * 1024 * 1024, // ... and the characters stored in it
  maxMembers: 100_000, // files/zip entries processed
  maxSummaryEntries: 500_000, // path/tag entries held across all members' summaries
  maxJsonDepth: 256, // nesting depth (the frame stack)
  maxTokenChars: 32 * 1024 * 1024, // one JSON string/scalar or one HTML text node/tag
});

export function resolveBudgets(overrides = {}) {
  const budgets = { ...DEFAULT_BUDGETS };
  for (const [name, value] of Object.entries(overrides)) {
    if (!(name in DEFAULT_BUDGETS)) throw new Error(`E_OPTION unknown budget ${name}`);
    if (!Number.isInteger(value) || value < 1) throw new Error(`E_OPTION budget ${name} must be a positive integer`);
    budgets[name] = value;
  }
  return Object.freeze(budgets);
}

const slots = entries => 2 ** Math.ceil(Math.log2(Math.max(16, entries * 2))); // open addressing keeps load <= 0.5

/**
 * Worst-case bytes held by input-dependent structures if EVERY budget is reached at once
 * (a ceiling, not an expectation; the measured typical figure is far lower). Hash tables are
 * typed arrays of 8 bytes per slot and briefly hold old + new tables while growing.
 */
export function memoryUpperBoundBytes(budgets = DEFAULT_BUDGETS) {
  const table = entries => slots(entries) * 8;
  const grow = bytes => Math.ceil(bytes * 1.5);
  return {
    leakProtectedTable: grow(table(budgets.maxProtectedPieces)),
    leakKeptTable: grow(table(budgets.maxKeptPieces)),
    identifierCounters: grow(table(budgets.maxDistinctIdentifiers)),
    unknownClassCounter: grow(table(budgets.maxUnknownClassTokens)),
    shortIdentifierTables: budgets.maxShortIdentifiers * 2 * 160 + budgets.maxShortIdentifierBytes * 4,
    memberRecords: budgets.maxMembers * 600,
    summaries: budgets.maxSummaryEntries * 220,
    jsonFrames: budgets.maxJsonDepth * 200,
    largestToken: budgets.maxTokenChars * 2 * 2, // UTF-16 plus one transient copy
  };
}

export function totalUpperBoundBytes(budgets = DEFAULT_BUDGETS) {
  return Object.values(memoryUpperBoundBytes(budgets)).reduce((sum, bytes) => sum + bytes, 0);
}
