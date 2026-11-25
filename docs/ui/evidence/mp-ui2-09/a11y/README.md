# MP-UI2-09 Connections: keyboard and accessibility qualification

Receipt: `receipt.json` (script `scripts/ui2-connections-a11y.mjs`).

**What was run:** an automated, keyboard-only pass in Chromium against the real UI2 shell
(real tokens and primitives) with a mocked native layer and synthetic data.

**Real defect found and fixed by this pass:** after toggling the sharing switch (and after
Create access or confirming a revoke) the page was rebuilt and keyboard focus fell to the
page body. `src/ui2/connections/ai-access.ts` now restores focus to the same control (opaque
keys, no ids in the page), or to the "Apps with access" heading when the control is gone.
Unit test: `keyboard focus stays put when the card re-renders` in `test/ui2-connections-page.test.ts`.

**Open observation for the owner (design system, not changed here):** the switch is 40x22 px
and the Advanced disclosure row is 19.5 px high, below the 24 px WCAG 2.2 target-size
minimum. The spacing exception was not evaluated. Changing it means changing a token or
primitive size, which this task is not allowed to do without an owner decision.

## Owner-only manual gates (NOT_RUN, not replaceable by this automated pass)

1. Keyboard-only walk-through of Connections in the packaged app (real WebView2): switch, Create
   access, Copy, Choose chats dialog, inline revoke (two steps), Advanced consent, EN and ID,
   narrow window, Lock, and focus return.
2. Screen reader pass (Narrator or NVDA): the switch name and state, the one-time key notice,
   dialog title and focus entry, the revoke question.
3. Windows high-contrast and 125 to 200 percent display scaling.
4. Decision on the two ledger deviations (no automatic-save switch in the app; no provider
   "Connected" tag) and on the target-size observation above.
