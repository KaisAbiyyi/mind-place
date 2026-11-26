# MP-UI2-06 design gap ledger: Archive and chat reader

Guide version implemented: `docs/ui/evidence/mp-ui2/wireframe-v1.html` (sha256 prefix `2b4bbce50a079516`).
Screens touched: Archive (list), chat reader, Info aside, Export aside. The shell gains a small page-to-shell hook (see "Shell wiring").
Status: written **before** the code (first commit of the branch). Every row below uses only the pattern catalog (`UI-PRINCIPLES.md` section 5) and the token set (section 7). No new colors, sizes, radii, spacings or control styles. The guide is not edited.

## What the guide does show (built as drawn)

- Archive page head with lead sentence and a primary **Import** button; provider chips (All, ChatGPT, Claude, Gemini); one card containing rows (title, one-line meta "Provider, saved date", one `Partial` tag); "Showing n of N"; empty states.
- Chat reader: back button, title, **Info**, **Copy**, **Export**; one-sentence partial banner with **Why?**; thread with the user bubble on the right and assistant text on the left (with a small provider initial).
- Info aside: groups Source, Saved copies, Completeness, and a collapsed **Technical details** (chat id, copy id).
- Breadcrumb `Home > Archive > chat title`.

## Design gap ledger (capability NOT shown in the guide, or deliberately different)

| # | Capability | Person's job (one sentence) | Class | Destination | Pattern used | Needs owner review |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | **Import entry is not usable yet** | "I want to add a ChatGPT or Gemini export to my archive." | Add (honest unavailable state) | Archive page head (as drawn, matrix row 22) | The drawn primary **Import** button, rendered **disabled**, with one plain sentence under the page head saying the feature is not available in this version. The `Transcript file (coming later)` boundary from the deferred table is part of the same sentence. No dialog is built (see "Import: why disabled"). | **YES** |
| 2 | Tool and system messages in the thread | "See that a tool or system step happened without it competing with the conversation." | Detail (thread) | Archive > chat | Left-aligned like assistant text, with a 12px muted role caption above (`Tool`, `System`); no bubble, no avatar. | **YES** |
| 3 | Pictures and files attached to a message | "See the pictures and files that were part of the chat." | Detail (thread) | Archive > chat | Image inside the message, captioned with its name (12px muted); other files are one muted line with the file name. Images load through the existing `get_asset` command with the same MIME allow-list and data-URL validation as the classic reader; failures say so in words. | y |
| 4 | Switching between saved copies | "Look at an earlier saved copy of this chat." | Connect (rows with a status tag and one action) | Archive > chat > Info aside > Saved copies | One row per copy (date, message count). The copy being shown carries the tag `Showing`; other rows have one `View this copy` button. First 8 rows, then `Show more`. Matrix row 4 moves the revision picker here. | y |
| 5 | Long chats | "Read a very long chat without the window freezing." | Browse (pagination) | Archive > chat | The list pager pattern (Previous, range status, Next), 40 messages per page, in a `Messages 1 to 40 of 312` status line. Focus goes to the first message of the new page. | y |
| 6 | Export result | "Save or copy this chat as a Markdown file." | Detail (aside) | Archive > chat > Export aside | The guide opens an aside with format buttons. Mine opens the same aside with one **Markdown** button (the only format the app can produce; plain text is covered by **Copy**). After running: either "Saved as a file" with the location, or "Ready" with `Copy Markdown` and `Save as...` (only when the window can offer a save dialog). Encrypted archives never write a file; the aside says so in one sentence. | y |
| 7 | How a copy was saved | "Know whether I saved this chat myself, it was saved automatically, or it came from an older archive." | Detail | Info aside > Source > How | One row of text: `Saved by you, from the browser`, `Saved automatically from the open page`, `Imported from an older Mind Place archive`. | no (extends the drawn `How` row) |
| 8 | Original page link | "Open the page this chat came from." | Detail | Info aside > Source | One row `Original page` with the host name as a link (http and https only, `noopener`). | no |
| 9 | Notes written while saving (`warnings`) | "Give support details if a save looked wrong." | Advanced | Info aside > Technical details (collapsed) | Appear only inside Technical details as one entry; never in a default view. | no |
| 10 | Provider names for filter chips | "Show only chats from one AI app." | Browse (chips) | Archive | The three drawn chips always exist; chips for any other provider found in the archive are added after them. Provider names are not translated. | no |
| 11 | Loading, empty, error, partial states | "Understand what is happening when a list is slow, empty or failed." | Empty, Warning, Progress | Archive, chat | UI-kit skeleton, empty state (ring icon, title, sentence, at most one action) and banner. List failure offers `Try again`. A chat that cannot be opened shows an empty state with `Back to Archive`. | no (matrix row 39) |
| 12 | Provider chrome text in the thread | "Read only what was said, not the button labels the website put around it." | Detail | Archive > chat (display layer only) | A pure display function removes a leading `Claude responded:`, `ChatGPT said:` or `Gemini said:` label and a trailing relative-time line (`just now`, `5 minutes ago`) from **assistant** messages. Stored evidence is never changed. The owner's own messages are never edited. | no |

## Import: why disabled (honest entry)

Backend `src-tauri/src/import_commands.rs` (`inspect_import_source`, `start_import`, `cancel_import`) exists on `main`, but it only accepts a backend-minted **selection grant**, and the command that mints one from a native dialog (the matrix `G6` picker, "arrives with MP-UI2-06") **does not exist**. This task may not touch `src-tauri/**`, so every import request would fail closed with `E_IMPORT_SELECTION`. The Import button is therefore shown as drawn but disabled, with a plain explanation. Nothing pretends to import. When the picker command lands, enabling it means adding the three-step dialog (Choose, Review, Import) and the encrypted-archive notice from the matrix; the button and its place do not move.

Owner question (non-blocking): is a disabled primary button with one explanatory sentence the right honest entry, or should the button be hidden until the picker command exists?

## Shell wiring (minimal)

- `Ui2PageContext` gains two **optional** members: `setDetail(detail | null)` (the page tells the shell the title of the open chat and what Back does) and `navigate(path)`. While a detail is set, the shell shows `Home > Archive > {title}` in the breadcrumb (the `Archive` crumb returns to the list) and lets the page own the heading. No route, destination or URL changes; chat ids never appear in the URL.
- `createUi2Pages` registers the `archive` page; `ensureUi2Styles` loads `src/ui2/chats/chats.css`.

## Not carried over from the classic Archive (and where it lives instead)

Title filter and completeness filter (global Search, MP-UI2-11); per-message "Copy answer" (Copy copies the whole chat); "Open Curated Memory" button (Memory owns the link to chats, matrix row 8); "Capture and import setup" button (Connections owns capture setup); legacy-archive and generic-transcript import buttons (Import entry above).

## Deviations from the guide (each needs owner approval)

1. Import button disabled with explanation (row 1).
2. User bubble corner radius uses the 14px `--r-lg` token instead of the guide's 18px, and the assistant initial uses `--r-pill` (tokens only; no new radius).
3. Export aside offers Markdown only (row 6); the guide's prototype also lists plain text.
4. Provider chips come from the guide's fixed three plus any other provider present (row 10).
