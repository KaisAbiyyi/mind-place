# MP-R1-WP01 · design gap ledger

Scope of UI change in this work package: **catalog strings only** (EN + ID). No screen, component, layout, token or style changes. The Import dialog that will show these strings is not built yet: it arrives with the native selection-grant picker (`G6`) after MP-UI2-06 (see PR #215, which shows the Import button as drawn but disabled).

## Gap 1 — Claude export in the Import kind list

| Step | Entry |
| --- | --- |
| Job | "I want to add the conversations from my Claude export to my archive." |
| Guide | The signed-off guide (`docs/ui/evidence/mp-ui2/wireframe-v1.html`, `importDialog`) lists ChatGPT export, Gemini (Google Takeout) and a transcript file. Claude is **not shown**. |
| Classification | Add. Destination: Archive → Import dialog, step 1 kind list (placement matrix row 22). No new destination, no new step. |
| Pattern | One more row of the existing kind list (`row` with title + one-line meta), identical to the ChatGPT and Gemini rows. No new control, color, size, radius or spacing. |
| Copy (EN) | `import.kind.claude` "Claude export" · `import.kind.claudeHint` "The .zip you download from Claude settings, under Privacy" |
| Copy (ID) | `import.kind.claude` "Ekspor Claude" · `import.kind.claudeHint` "File .zip yang kamu unduh dari pengaturan Claude, di bagian Privasi" |
| Sibling rows | `import.kind.chatgpt(Hint)` and `import.kind.gemini(Hint)` copied verbatim (EN and ID) from the guide's `imp.k.*` strings so the list is complete in the catalog. |
| Technical detail | None in the default view. Member names, counts and reasons stay in the receipt (Details/Info), per `UI-PRINCIPLES.md`. |
| Screenshots | **NOT_RUN** — there is no rendered surface yet. Required (light/dark × EN/ID × wide/laptop/narrow) in the PR that builds the Import dialog. |

## Gap 2 — honest-limits note for Claude

| Step | Entry |
| --- | --- |
| Job | "Before I import, tell me plainly what a Claude export can and cannot give me." |
| Guide | The review step shows `imp.note` ("Some exports hold only the active branch of a conversation. Anything missing is marked, never invented."). Nothing Claude-specific. |
| Classification | Warning (informational), same place as `imp.note` on the Review step, shown only when the kind is Claude. |
| Pattern | Same small muted paragraph as `imp.note`. |
| Copy (EN) | `import.kind.claudeNote` "Claude exports do not say which model answered. Chats with edited messages are kept safely but may not appear in your archive yet. Anything missing is marked, never invented." |
| Copy (ID) | `import.kind.claudeNote` "Ekspor Claude tidak menyebut model yang menjawab. Obrolan dengan pesan yang diedit tetap disimpan dengan aman, tetapi mungkin belum muncul di arsip. Yang tidak ada ditandai, tidak pernah dikarang." |
| Why this wording | A Claude chat whose message links fork (an edited message) is kept as raw evidence only, so it does not appear in the reader yet. The first draft ("may not show edited branches") understated that (independent review, P2). |
| Screenshots | **NOT_RUN** (as above). |

## Also changed

- `import.subtitle` and `import.chooseFile` now name Claude (EN and ID).

## Stale statements elsewhere (owner to reconcile, not edited here)

- `docs/ui/MP-UI2-placement-matrix.md` rows 22 and the transcript-file gap still say the backend accepts only `chatgpt` and `gemini`. After this PR `check_provider` also accepts `claude`. The matrix is part of the `MP-UI2-00` contract, so it is left for the owner to update.
