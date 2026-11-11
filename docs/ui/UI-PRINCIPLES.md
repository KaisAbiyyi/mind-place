# Mind Place UI principles — the "soul" of the interface

Status: **draft for owner approval (2026-11-11)**. Once approved this file is binding for every UI task, human or agent. It exists because a recurring failure happened: an HTML design guide was incomplete, an agent implemented capabilities the guide never showed, improvised new layouts, and the product turned ugly again. This file tells agents what to do **when the guide is silent**.

Related: [MP-UI2 migration plan](MP-UI2-redesign-migration-plan.md), the signed-off HTML guide (see "Ground truth" below), `docs/ui/MP-UI-09-feature-coverage-matrix.md`.

## 1. Who the UI is for

The interface is for **people**, in this order: the owner reading and tidying their own knowledge, then anyone they allow to look over their shoulder. It is **not** a debugging console for agents. Agents get APIs, MCP and the technical-details panels. The default screen must make sense to a person who has never read the architecture documents.

Three words describe the feel: **calm, plain, tidy.** If a change makes a screen louder, denser or more technical, it is wrong even when it is correct.

## 2. Authority order when sources disagree

1. The owner's explicit written decision (chat, Notion decision log, PR review).
2. The signed-off **HTML guide** for anything it shows.
3. **This file** plus the design tokens for everything the guide does not show.
4. Existing code. Existing code is never evidence of what the design should be.

The older instruction "preserve existing features absent from the prototype by extending its closest layout/component/interaction patterns" is **replaced** by the gap protocol below. "Closest pattern" left too much judgement and produced inconsistent screens.

## 3. Ground truth and versioning

- The guide lives **in the repository** (for example `docs/ui/evidence/mp-ui2/wireframe-v1.html`) with a SHA-256 in its manifest. A guide that exists only in a chat, an artifact link or a design tool is not ground truth for cloud agents.
- A PR states which guide version it implements. Agents never edit the guide to make their implementation look right. A guide change is an owner-approved change of its own.
- The guide is intentionally incomplete. Incomplete is normal; improvising is not.

## 4. The gap protocol (mandatory when a capability is not in the guide)

Do these in order. If a step fails, **stop and mark the task `BLOCKED-DESIGN`** with a question for the owner. Do not ship a guess.

1. **State the person's job.** One sentence: what is a person trying to do with this capability? ("Allow an AI app to read my memory.") If you cannot say it without technical terms, the feature is not ready for the default view.
2. **Classify it** with the table in section 5 (browse, detail, add, configure, connect, status, revoke, advanced, empty, warning, progress).
3. **Place it** using the destination map in section 6. **Never add a top-level destination.** The sidebar has seven; changing that needs an owner decision.
4. **Build it only from the pattern catalog** in section 5 and the tokens in section 7. No new colors, font sizes, radii, shadows, spacings or control styles.
5. **Write the copy** with section 8 and put every string through the i18n layer (EN + ID).
6. **Hide what is technical.** Identifiers, hashes, revisions, scores, protocol names and raw JSON never appear in the default view. They live behind a Details/Info control or technical mode.
7. **Record it** in the PR "design gap ledger" (section 10) with screenshots. A capability with no entry in the ledger is not allowed to merge.
8. If **no catalog pattern fits**, do not invent one. Mark `BLOCKED-DESIGN`, describe the job, and propose at most two options as text, not code.

## 5. Pattern catalog (the only building blocks)

| Class | Pattern | Rules |
| --- | --- | --- |
| Browse | Card containing a list of rows (title, one-line meta, optional tag, chevron-free). Memory may also offer a card grid. | Max one tag per row by default. Title 14/500, meta 13 muted. Paginate or virtualize; never dump everything. |
| Detail | **Full page** with breadcrumb, one lead sentence, then sections. Metadata and provenance go in an **Info aside** (grouped, with "Technical details" collapsed inside it). | Chat detail looks like a chat app: user bubble right, assistant text left, nothing competing with the thread. |
| Add / create | **Dialog, single column, with steps** (Choose, Review, Progress). One primary button, one cancel. | Native pickers where a path or file is involved. Nothing is read or changed before the Review step is approved. Field label above the field, help text below it, never beside it. |
| Configure | **Settings row**: title (14/500), one-line description (13 muted), control on the right (switch, segmented, button). | Groups are cards. No more than about seven rows per card. |
| Connect | List of rows with a status tag and one action. | Status words are plain ("Connected", "Needs setup"). |
| Status / health | A tag plus one sentence. | Unknown is shown as unknown, never as zero or healthy. |
| Danger / revoke / remove | **Inline two-step**: the action button turns into Cancel + Confirm in place. | No `window.confirm`, no modal on top of a modal. Say what is not affected ("Your files are never deleted."). |
| Advanced | A collapsed **disclosure** at the bottom of its card, titled "Advanced: ...". | Collapsed by default. Never required for the normal path. |
| Empty | Ring icon, title, one or two sentences, **one** action. | Says what will appear and why it is empty. Never a blank area. |
| Warning / partial | Single-sentence banner with a "Why?" link that opens the Info aside. | One banner per view. |
| Progress / long running | Inline tag with percentage, and Stop where stopping is safe. | Never block the whole screen. |
| Search | Command-palette dialog, scope chips (Everything, Memory, Chats, Files), results grouped with **Memory first**, select-to-share bar. | Advanced retrieval options sit behind a disclosure. |

Anything not in this table is not allowed until the table is extended by the owner.

## 6. Destination map (where things live)

Seven destinations, in this order: **Home, Memory, Chats, Constellation (working name), Sources, Connections, Settings.**

- **Home**: latest memories first, recent chats, and the setup checklist **only until it is complete** (then it disappears; Settings can bring it back).
- **Memory**: the tidy, curated knowledge. Topics, Projects, People. Read-first.
- **Chats**: the raw saved conversations (the "messy place"). Reader, copies, export, import of transcripts (placement pending owner decision).
- **Constellation**: graph of curated memory only; empty state when there is no memory; no seed input.
- **Sources**: folders and files Mind Place may read; add via native pickers.
- **Connections**: how chats get in (browser capture, automatic save, registration) and which AI apps may read memory. Advanced hosted connection is collapsed here.
- **Settings**: appearance, language, privacy and lock, encryption and backup, maintenance, recovery.

When a new capability fits two places, choose the one where a person would look **when they want to do the job**, not where the code lives.

## 7. Visual system (tokens only)

- **Look**: monochrome zinc neutrals, near-black primary, white surfaces; light and dark themes designed together. Semantic color only for ok (green) and warn (amber), used sparingly.
- **Type scale** (px): 12, 13, 14 (base), 16, 20, 28. Weights 400, 500, 600. One family (Inter, with system fallback). Page title 28/600, section title 16/600, body 14/400, meta 13 muted, caption 12 faint. Do not add sizes.
- **Spacing scale** (px): 4, 8, 12, 16, 24, 32, 48. Use layout `gap`, not ad-hoc margins.
- **Radius**: 7 (controls), 10 (cards), 14 (dialogs). **Shadow** only on floating layers (aside, dialog, toast).
- **Page width**: 880 for lists and dashboards, 720 for reading and settings. Aside 400. Dialog 620. Sidebar 232 (64 collapsed). Top bar 56.
- **Shell**: sidebar header holds only the logo; the toggle lives in the top bar; the top bar also holds the breadcrumb, search, language, lock. Breadcrumb shows the nested path and every part is a link.
- **Density**: one primary action per page head or dialog. No more than one row of controls above a list. Prefer fewer, larger targets over many small ones. If a screen needs a toolbar of more than four controls, it is doing too much.
- **Motion**: minimal; respect reduced motion.
- **Forbidden**: new hex colors, new font sizes, gradients, nested cards, cards inside cards inside cards, icons-as-decoration, emoji, tiny inputs, helper text beside inputs, text-only walls longer than about 60 characters per line.

## 8. Words (copy and i18n)

- Every user-visible string goes through the i18n layer. **English and Indonesian ship together**; adding a language means adding a catalog, not touching components. Plurals and dates use `Intl`. Never build sentences by concatenation.
- Product names (ChatGPT, Claude, Gemini, Mind Place) are not translated. The owner's own chat text is never translated.
- Write from the person's side: what they recognise, not how the system is built. Active voice. A button says exactly what happens. Errors say what went wrong and what to do next, without apology.
- Translate technical vocabulary into human words (glossary):

| Internal term | Say instead (EN) | Bilang (ID) |
| --- | --- | --- |
| snapshot / capture revision | saved copy | salinan tersimpan |
| conversation source / locator / canonical key | (hidden; shown only under Technical details) | (disembunyikan) |
| MCP listener / reader credential | AI app access / access key | akses aplikasi AI / kunci akses |
| hosted tunnel | hosted connection | koneksi hosted |
| index rebuild | rebuild search | bangun ulang pencarian |
| approved root | shared folder | folder yang dibagikan |
| provenance | where this came from | asalnya dari mana |
| partial capture | this copy may be missing messages | salinan ini mungkin kehilangan pesan |
| vault mode | encryption | enkripsi |

## 9. Accessibility and states

- Everything works with keyboard alone; focus is visible; dialogs and asides trap focus and return it on close; Escape closes the top layer.
- Every interactive element has an accessible name; graphs offer an accessible list alternative.
- Contrast holds in both themes. Do not rely on color alone for state.
- Every view has designed **loading, empty, error, partial and stale** states, written in plain words. Unknown is never shown as zero.
- Existing security behaviour is part of the UI contract and is not redesigned: locked by default, lock destroys the reader and fences late work, recovery only on the locked gate, no secrets in URLs or logs, sanitized transcripts only.

## 10. What every UI pull request must contain

Paste this block into the PR description and fill it in:

```text
Guide version implemented: <file + sha256 or tag>
Screens touched: <list>
Design gap ledger (one row per capability NOT shown in the guide):
  capability | person's job | class (section 5) | destination (section 6) | pattern used | needs owner review (y/n)
Evidence: screenshots in light + dark, EN + ID, three widths (wide, laptop, narrow)
Technical strings visible in default view: none  (list any exception and why)
i18n: no missing keys, no raw strings
Accessibility: keyboard pass notes, focus return checked
Deviations from the guide: none  (or list; each needs owner approval)
```

Automated checks (added by the design-system task) fail the build on: hex colors or font sizes outside the token set, raw user-facing strings in v2 modules, missing i18n keys, and technical identifiers rendered outside a Details/Info container.

## 11. Anti-patterns that made the earlier UI look bad (do not repeat)

- Raw field dumps and long technical paragraphs shown by default.
- Hashes, revision ids and protocol names as visible labels.
- Many small controls in one row; sliders and selects for things nobody asked to tune.
- A "seed" or "ID" input before anything useful is shown.
- A list of fourteen near-identical rows where grouping and collapse would do.
- Helper text beside a cramped input; tiny inputs; truncated placeholders.
- Inventing a new layout for the one feature the guide forgot.
- Showing a graph or table of data nobody curated.

## 12. Changing this file

Only the owner approves changes. Propose changes in a PR that updates this file, the guide manifest if affected, and the pattern catalog together.
