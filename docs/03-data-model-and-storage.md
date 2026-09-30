# Data Model and Storage

## 1. Storage objectives

The storage layer should be:

- local-first;
- compact;
- durable;
- inspectable;
- model-independent;
- provider-independent;
- easy to back up;
- easy to migrate;
- capable of full-text search;
- capable of representing provenance and graph relationships;
- efficient enough to retain complete conversation history indefinitely.

## 2. Initial storage recommendation

Start with:

~~~text
SQLite
  structured metadata
  conversations
  messages
  memories
  graph
  indexes

SQLite FTS5
  lexical full-text index

objects/
  content-addressed files and attachments
~~~

Potential later additions:

- Zstandard compression for large raw text payloads;
- sqlite-vec or another vector extension;
- compressed content blobs;
- derived cache tables.

The initial system does not need PostgreSQL, a dedicated graph database, or a dedicated vector database.

## 3. Why SQLite

SQLite matches the product's local-first nature:

- single-file portability;
- excellent transactional guarantees;
- very low deployment complexity;
- mature FTS5;
- adequate scale for personal data;
- easy backup;
- compatible with desktop and local services;
- works well with TypeScript and native applications.

The graph can initially be represented with normal node and edge tables.

## 4. Provider-independent conversation schema

Do not store provider HTML or browser DOM as canonical conversation data.

A normalized message could conceptually look like:

~~~json
{
  "id": "msg_local_...",
  "provider": "chatgpt",
  "provider_message_id": "...",
  "conversation_id": "...",
  "parent_id": "...",
  "role": "assistant",
  "created_at": "...",
  "model": "provider-model-name",
  "parts": [
    {
      "type": "markdown",
      "content": "..."
    }
  ]
}
~~~

The exact schema can evolve, but the representation should model semantics rather than presentation markup.

## 5. Suggested core entities

### 5.1 conversations

Candidate fields:

~~~text
id
provider
provider_conversation_id
title
created_at
updated_at
captured_at
is_temporary
capture_status
metadata_json
~~~

### 5.2 messages

~~~text
id
conversation_id
provider_message_id
parent_message_id
role
model
created_at
completed_at
content_hash
content_json or content_blob_ref
branch_key
metadata_json
~~~

### 5.3 message parts

Possible types:

- markdown;
- plain text;
- code;
- citation;
- tool call;
- tool result;
- image reference;
- file reference;
- structured data.

It may be more compact to keep parts in a serialized payload at first.

### 5.4 attachments

~~~text
id
sha256
mime_type
size_bytes
object_path
original_name
created_at
metadata_json
~~~

Use the content hash as the deduplication primitive.

Two conversations attaching the same file should be able to reference the same stored object.

### 5.5 memories

~~~text
id
content
type
topic
status
created_at
updated_at
valid_from
valid_until
supersedes_memory_id
created_by
metadata_json
~~~

Not every field must exist in V1. The key idea is to support durable content plus history and provenance.

### 5.6 memory sources

Many-to-many provenance:

~~~text
memory_id
source_type
source_id
source_range_json
relationship
~~~

A memory may be supported by multiple messages or files.

### 5.7 graph nodes

~~~text
id
node_type
label
canonical_key
source_type
source_id
metadata_json
created_at
updated_at
~~~

### 5.8 graph edges

~~~text
id
from_node_id
to_node_id
relation
weight
origin
source_id
metadata_json
created_at
~~~

The origin field can distinguish:

- manual;
- model-authored;
- imported;
- deterministic;
- inferred.

## 6. Raw versus derived storage

The database should make this distinction explicit.

### Raw or canonical

- captured message content;
- original file bytes;
- source timestamps;
- provider IDs;
- user-authored memory;
- model-authored curated memory as an explicit authored record.

### Derived or rebuildable

- FTS chunks;
- embeddings;
- summaries;
- topic labels;
- automatically inferred graph edges;
- generated context packs;
- ranking scores.

A useful design rule:

> If deleting a table would permanently destroy original information, it is canonical. Otherwise it should be treated as derived.

## 7. Compression strategy

Conversation text compresses well because natural language and code contain repeated structure.

Possible strategy:

1. Keep frequently accessed metadata uncompressed.
2. Index searchable text through FTS.
3. Compress large raw payloads with Zstandard.
4. Fetch and decompress only when exact source text is requested.

Do not prematurely compress small rows if it complicates search and updates.

A threshold-based approach is reasonable.

## 8. Size expectations

The exact bytes-per-token ratio varies, but raw conversation text is normally cheap enough to retain aggressively.

Rough normalized JSON estimates:

| Tokens | JSON size | Compressed range |
| ---: | ---: | ---: |
| 10K | 40-60 KB | 10-25 KB |
| 50K | 200-300 KB | 50-120 KB |
| 100K | 400-600 KB | 100-250 KB |
| 500K | 2-3 MB | 0.5-1.2 MB |
| 1M | 4-6 MB | 1-2.5 MB |

Attachments will dominate total size much sooner.

Storage policy should therefore distinguish:

- textual conversation retention: effectively unlimited by default;
- file retention: configurable;
- large media: perhaps reference-only, deduplicated, or opt-in.

## 9. Idempotency

Automatic capture must tolerate repeated events.

Recommended uniqueness strategy:

~~~text
(provider, provider_conversation_id, provider_message_id)
~~~

If provider IDs are unavailable or unstable, combine:

- conversation identity;
- role;
- timestamp;
- normalized content hash;
- parent identity.

Capture logic should be able to run twice without duplicating the same message.

## 10. Branches and regenerations

Do not flatten retries into one reply.

A user may regenerate an answer or edit an earlier prompt.

Represent the conversation as a tree or DAG where possible:

~~~text
user message A
   |
   +-- assistant reply B1
   |
   +-- assistant reply B2
~~~

Likewise, editing a prior prompt creates a new branch rather than rewriting history.

The UI may show only the active branch by default while retaining alternatives.

## 11. Provenance

Every important derived or curated artifact should answer:

- Where did this come from?
- Was it created manually or inferred?
- Which exact source supports it?
- When was it created?
- Which model or tool created it, if applicable?
- Has it been superseded?

Provenance turns the system from a pile of summaries into an auditable personal knowledge store.

## 12. Backup and portability

Long-term design should support:

- copying the SQLite database;
- copying the object directory;
- exporting normalized JSON or JSONL;
- restoring on another machine;
- re-indexing derived layers.

A complete backup should not require any external AI provider to be readable.

## 13. Security direction

The data may become highly sensitive.

Future implementation should consider:

- binding APIs to localhost by default;
- per-client authentication tokens;
- optional database and object encryption;
- operating-system credential storage;
- explicit capture indicators;
- temporary-chat detection;
- fast global capture pause;
- secure deletion semantics for user-requested removals.

Security is not optional simply because the system is local.
