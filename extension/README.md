# Mind Place capture extension

This Chromium Manifest V3 extension saves one explicitly selected ChatGPT conversation to the local Mind Place archive. Opening the popup does not read the page. **Save this conversation** starts the capture; **Load older messages by scrolling** is on by default and can be unchecked for a quick DOM capture.

## Optional automatic saving (1.3.0)

Automatic saving starts **OFF**. Enable **Automatically save loaded messages** and grant the optional `https://chatgpt.com/*` access in the popup. It attaches existing and newly opened regular ChatGPT tabs, observes loaded conversation changes and new conversation URLs, then captures only rendered messages. It **never scrolls**, changes scroll styling/position, or loads older messages. Quiet changes wait 1.2 seconds; ongoing streaming is checkpointed at most once per five-second window. Unchanged content deduplicates and idle pages hold no native process open.

Observation stays in the isolated top frame, scoped to the visible conversation surface. Navigation waits for retained old message identities to disappear; fallback positional keys are not treated as stable. Explicit mismatching DOM conversation ownership is rejected. Automatic saving pauses during manual history loading. Disabling or revoking access removes dynamic script registration, stops current observers and prevents unsubmitted saves. Already submitted native writes cannot be recalled. There is no timed native retry/reconnect loop.

Automatic captures use `automatic-dom`/partial provenance and store that loaded window as a new immutable revision. A smaller latest window does not delete previous larger/manual revisions; choose them in the reader's saved-capture selector. Windows are not merged into an invented transcript. Previously evidenced original dates remain in archive-list sorting when an automatic window lacks newer original metadata. A selected revision has its own dates. The popup exposes the last durable outcome or error.

The extension accepts only `https://chatgpt.com/c/<UUID>` and `https://chatgpt.com/g/<project>/c/<UUID>` pages. It refuses private windows and proven Temporary Chat. Scrolling waits up to five seconds for the conversation's real scroll area to mount and receive layout, loads the newest/bottom window, then makes one upward sweep in overlapping viewport steps. It accumulates virtualized message windows, waits for visible history loaders and changing content, and restores the original scroll offset when possible. Production pacing uses 40 ms checks, 80 ms quiet intervals and a 750 ms oldest-boundary interval. A moved initial bottom uses that longer interval so ChatGPT's initial scroll restoration can settle; an existing bottom keeps the short wait. Loading indicators, changing content and lagging rendered windows extend waits within the three-minute budget. It always records partial completeness: reaching a stable scroll boundary does not prove all history, branches, or remote attachments are present. Remote media remain references; no remote media or provider API is fetched. Small safe inline raster image data may be copied into the capture.

Native message timestamps are preserved separately from capture time. Current ChatGPT places a turn's original user time in a separator outside the message units; this is retained as user `authored_at`, without inventing an assistant send time. Optional provider creation/update values require an exact conversation identity on the active DOM ancestor. The reader sorts by original activity metadata; unavailable dates remain explicit. Old snapshots without these dates stay immutable and can be supplemented by a new manual capture.

The popup and the conversation page show progress with **Stop & save** and **Cancel**. Stop saves the history collected so far. Cancel sends nothing to the archive. Closing the popup does not cancel manual scrolling; an event-driven worker owns the explicit job with a temporary native connection. Manual jobs do not retry automatically. Loading ends at stable boundaries, Stop, Cancel, navigation/privacy changes, size failure, or the three-minute budget; a time-limited result keeps an explicit warning. Once the native save is submitted, its durable receipt determines the result.

## Load for development

1. Open `chrome://extensions` (or `edge://extensions`) and enable Developer mode.
2. Choose **Load unpacked** and select this `extension/` directory.
3. Open Mind Place → Settings → Manual capture setup and register this extension in Chrome or Edge.
4. Open a regular ChatGPT conversation, click the Mind Place extension button, and choose **Save this conversation**.

After updating the unpacked extension, choose **Reload** in the browser's extension manager. The extension ID and native host name are unchanged. Chromium 105+ is required for the native port's worker lifetime behavior.

The manifest public key is held in `public-key.txt` and fixes the extension ID to `dninfeidggaaioioficnigfpfjkdicjh`. The native host must register that matching identity and return the Rust contract's durable `SaveReceipt`. A missing host is reported in the popup with the Mind Place setup location.

## Capture boundary

Required permissions are `activeTab`, `scripting`, `nativeMessaging`, and `storage`. Only optional ChatGPT origin access is declared; no unconditional content script or mandatory host access is shipped. Navigation/change observation is enabled only by the automatic toggle and granted origin access. Neither mode calls provider APIs, uses browser debugging, or reloads tabs. Exact URL/privacy identity is validated before native submission to `com.mindplace.capture`. Only a valid durable save receipt counts as success. A timed-out or lost acknowledgement may have committed; check the archive before retrying.

Limits remain 4 MiB per message, 10,000 collected messages, 31 MiB capture JSON, 120 attachment references, and 16 MiB inline image data across the collection. Attachment-reference omissions are explicit warnings; message text is retained. Missing stable message/turn identity produces a warning about repeated turns and ordering. Historical raw revisions remain immutable; this extension does not erase or replace older snapshots.

## Provider adapters

Manual and opt-in automatic saving dispatch through `providers/index.mjs`.
ChatGPT is the only registered provider; `capture.mjs` re-exports its serialized
capture helpers for compatibility. See
[`phase-7a-capture-adapters.md`](../docs/implementation-plan/phase-7a-capture-adapters.md)
for the versioned interface, normalized evidence contract and qualification limits.
