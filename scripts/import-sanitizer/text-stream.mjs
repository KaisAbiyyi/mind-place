// Bounded line reader for plain-text members. The pending (not yet newline-terminated) text can
// never exceed the token budget: a longer line stops the run with BudgetError before any
// concatenation that would grow past it, instead of accumulating an unbounded buffer.
import { StringDecoder } from 'node:string_decoder';
import { BudgetError, DEFAULT_BUDGETS } from './budgets.mjs';

/** Yields { line, newline } for each line. Decodes UTF-8 across chunk boundaries. */
export async function* boundedLines(readable, { maxTokenChars = DEFAULT_BUDGETS.maxTokenChars } = {}) {
  const decoder = new StringDecoder('utf8');
  const over = () => { throw new BudgetError('maxTokenChars', maxTokenChars); };
  let carry = '';
  for await (const chunk of readable) {
    const text = decoder.write(chunk);
    let start = 0;
    for (;;) {
      const newline = text.indexOf('\n', start);
      if (newline < 0) break;
      if (carry.length + (newline - start) > maxTokenChars) over(); // checked BEFORE the concatenation
      yield { line: carry + text.slice(start, newline), newline: true };
      carry = '';
      start = newline + 1;
    }
    if (carry.length + (text.length - start) > maxTokenChars) over();
    carry += text.slice(start);
  }
  const tail = decoder.end();
  if (carry.length + tail.length > maxTokenChars) over();
  carry += tail;
  if (carry) yield { line: carry, newline: false };
}
