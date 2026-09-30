# Conversation Capture and Ingestion

## 1. Capture philosophy

Mind Place should preserve the full history of ordinary AI conversations automatically.

The user should not need to manually decide after every message whether the conversation is worth keeping.

The default flow for normal chats is:

~~~text
user submits prompt
      |
capture user turn
      |
assistant streams response
      |
response finishes
      |
capture completed assistant turn
~~~

The extension should not store every streamed token as a separate event.

## 2. Browser extension role

A browser extension is the practical bridge while hosted chat products have inconsistent support for writable custom MCP tools.

The extension has two important responsibilities:

1. Automatic archival.
2. Optional UI bridge.

Automatic archival does not depend on the AI model deciding to call a tool.

This matters because raw continuity should not be lost merely because one provider cannot write to a local MCP server.

## 3. Provider adapters

Each provider UI should have a small adapter:

- ChatGPT adapter;
- Claude adapter;
- Gemini adapter;
- future provider adapters.

The adapter maps provider-specific page state into a stable internal event.

Illustrative shape:

~~~text
CapturedMessage
  provider
  conversationId
  messageId
  parentId
  role
  model
  createdAt
  completedAt
  parts
  metadata
~~~

## 4. Do not use DOM as the archive

DOM inspection may be necessary to capture data from a web app.

DOM output itself should not become canonical storage.

Problems with storing raw DOM:

- huge overhead;
- presentation noise;
- provider redesigns make it meaningless;
- class names are unstable;
- difficult to search;
- permanently provider-specific.

Instead, parse it into normalized semantic content.

## 5. Preferred capture sources

In order of preference:

1. Stable provider or API state exposed to the page.
2. Network responses when technically appropriate and robust.
3. Structured page state.
4. Semantic DOM extraction.
5. Last-resort presentation scraping.

The adapter should be replaceable without changing stored data.

## 6. Temporary and private chats

This is a hard privacy rule:

> If the provider explicitly marks the conversation as temporary, private, or non-history, automatic capture should default to OFF.

Expected behavior:

~~~text
temporary chat detected
        |
capture disabled
        |
clear visible indication
~~~

There should also be:

- pause capture for this conversation;
- global pause;
- optional explicit override if the user consciously chooses to save a temporary chat.

The default must respect the user's privacy intent.

## 7. Response completion

Do not commit a new database version for every streamed token.

Suggested flow:

~~~text
assistant starts
   |
buffer or observe
   |
assistant completes
   |
normalize final response
   |
hash and idempotency check
   |
store
~~~

If the browser closes mid-stream, the extension may optionally preserve a partial message marked as incomplete, but incomplete-state handling can be deferred from V1.

## 8. Regeneration and edited prompts

AI products allow:

- regenerate;
- retry;
- edit earlier prompt;
- branch from earlier message.

Do not overwrite earlier responses.

If provider structure is available, preserve the branch:

~~~text
prompt P1
  |
  +-- response R1
  |
  +-- response R2

edited prompt P1-prime
  |
  +-- response R3
~~~

This matters because an abandoned answer can later contain useful information.

## 9. Deletion semantics

A future design decision is required for provider-side deletion.

Possible policy A: local archive independence.

Deleting a provider chat does not automatically delete local history.

Pros:
- archive is truly independent.

Cons:
- may violate user expectation if deletion is assumed global.

Possible policy B: mirrored deletion.

Detect provider deletion and offer or perform local deletion.

Pros:
- intuitive.

Cons:
- technically unreliable across providers.

A safer product direction may be:

- local archive is independent by default;
- UI communicates that clearly;
- provide easy local deletion;
- never silently infer deletion intent.

## 10. Importing historical exports

The extension only captures future activity.

Mind Place should also support importers for:

- exported ChatGPT conversations;
- exported Claude conversations;
- exported Gemini activity where available;
- plain JSON;
- Markdown;
- text;
- HTML after normalization;
- future provider exports.

Importers should produce the same canonical data model as live capture.

## 11. File ingestion

Any file may become a source in Mind Place.

Examples:

- Markdown design document;
- PDF;
- code file;
- repository snapshot;
- image;
- spreadsheet;
- chat export;
- transcript.

The raw object is stored once, ideally content-addressed:

~~~text
sha256(file bytes)
      |
objects/<hash>
~~~

Structured metadata in SQLite references the object.

Text extraction and chunking are derived data and can be regenerated.

## 12. Conversation attachments

When a conversation includes an attachment:

1. Create or reuse the attachment object.
2. Link the message to the attachment.
3. Preserve filename and MIME metadata.
4. Avoid embedding the binary as base64 in conversation JSON.
5. Deduplicate by hash.

This keeps conversation records small.

## 13. Capture health

The extension should eventually expose capture health because provider UIs change.

Possible states:

- healthy;
- provider adapter outdated;
- unable to identify conversation;
- unable to detect completion;
- temporary chat or disabled;
- server unreachable;
- queueing locally.

Silent capture failure is unacceptable for a continuity product.

## 14. Offline or daemon-unavailable behavior

If the local service is temporarily unavailable, the extension could maintain a small durable browser-side queue and retry.

Requirements:

- bounded queue;
- idempotent replay;
- clear error indicator;
- no duplicate messages after reconnection.

This can be added after the basic local capture path works.
