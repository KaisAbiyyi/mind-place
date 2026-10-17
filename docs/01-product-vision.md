# Product Vision and Principles

## 1. Problem statement

AI model quality changes in cycles. OpenAI, Anthropic, Google, and future providers may continuously overtake each other in coding, reasoning, research, tool use, speed, or cost.

That creates an undesirable dependency:

~~~text
better model appears
      |
      v
user switches provider
      |
      v
personal continuity resets
~~~

The value accumulated through months or years of conversations should not be owned by whichever model happens to be best this month.

A person may have project history in ChatGPT, coding decisions in Claude, research context in Gemini, local files that none of them permanently know, small preferences that become useful much later, and obscure details that were never important enough to become manually curated memories.

The core problem is therefore not "how do we build another chatbot memory feature?"

It is:

> How do we give a person a durable, local continuity layer that survives changes in AI provider, model generation, product UI, and memory implementation?

## 2. Product thesis

Mind Place separates two things hosted AI products usually bundle together:

1. Model intelligence.
2. Personal continuity.

Model intelligence is rented and replaceable.

Personal continuity should be owned.

~~~text
          DURABLE
      personal context
             |
      +------+------+
      |      |      |
     GPT   Claude Gemini
      |      |      |
      +------|------+
             |
         replaceable
~~~

A future model should be able to connect to Mind Place and immediately research relevant past context without requiring the user to retell their history.

## 3. The memory philosophy

A major inspiration is the behavior of high-quality assistant memory systems: the assistant itself can decide that a detail is worth remembering, decide how to phrase it, and later search old context when an apparently unimportant detail becomes relevant.

Mind Place should preserve that division of responsibility.

The storage engine does not need to decide whether a statement matters.

The active model can decide:

- whether something deserves a curated memory;
- how that memory should be represented;
- whether an older memory should be updated;
- which memories or source conversations are relevant;
- how deeply it needs to research historical context.

The core supplies reliable primitives and preserves evidence.

This can be described as model-managed shared memory over a model-independent local store.

## 4. Why save raw conversations as well as curated memory?

Curated memory alone loses too much.

Small details are valuable precisely because their future importance is unpredictable.

Examples:

- a minor preference mentioned once;
- an architectural decision whose consequences become relevant months later;
- an exact wording from a previous discussion;
- an abandoned idea that later becomes useful;
- a temporary constraint that explains why an old decision was made;
- details that were too unimportant for the model to save as long-term memory at the time.

Therefore Mind Place should preserve two parallel forms of memory.

### 4.1 Raw history

High-fidelity source material:

- complete conversations;
- message ordering;
- branches and regenerations when available;
- provider metadata;
- files;
- source timestamps;
- citations and links;
- attachment references.

### 4.2 Curated memory

Compact long-lived statements selected by a model or user:

- preferences;
- project state;
- recurring workflows;
- decisions;
- stable facts;
- durable constraints.

Neither replaces the other.

Curated memory provides fast continuity.

Raw history provides recoverability and deep search.

## 5. Storage is not the primary bottleneck

Pure text is cheap compared with modern local storage.

A useful rough design estimate for normalized text conversations is:

| Conversation size | Approx. normalized JSON | Approx. compressed |
| ---: | ---: | ---: |
| 10K tokens | 40-60 KB | 10-25 KB |
| 50K tokens | 200-300 KB | 50-120 KB |
| 100K tokens | 400-600 KB | 100-250 KB |
| 500K tokens | 2-3 MB | 0.5-1.2 MB |
| 1M tokens | 4-6 MB | 1-2.5 MB |

These are architectural estimates, not protocol guarantees. Different languages, code density, metadata, and formatting change the ratio.

Even an extreme hypothetical of one million new text tokens every day is only on the order of a few gigabytes per year before aggressive compression.

Attachments dominate storage far sooner than text.

Therefore the design should prefer:

> preserve first, optimize retrieval second.

It should not delete raw conversation detail merely to save a few megabytes.

## 6. Product principles

### 6.1 Local first

The canonical store should live on the user's machine by default.

Benefits:

- ownership;
- low recurring storage cost;
- privacy;
- offline browsing;
- direct integration with local developer tools;
- independence from provider account retention policies.

Cloud synchronization may exist later, but it should be additive rather than foundational.

### 6.2 Provider neutral

No internal representation should assume ChatGPT, Claude, Gemini, or any single provider.

Provider-specific fields belong in adapters or optional metadata.

The canonical concepts are things like:

- conversation;
- message;
- content part;
- attachment;
- memory;
- source;
- graph node;
- graph edge.

### 6.3 Raw data is sacred

Raw imported or captured source should be append-only or immutable wherever practical.

A summarizer, indexer, graph builder, or future model must never be able to silently rewrite historical truth.

### 6.4 Derived data is disposable

Search indexes, embeddings, summaries, topic clusters, inferred relationships, and graph enrichments should all be reproducible.

If a better model appears tomorrow, derived layers can be deleted and rebuilt.

### 6.5 Intelligence belongs at the edge

The core should remain useful with no local LLM installed.

A connected model can provide intelligence through MCP or other tool calls.

Optional maintenance workers can enrich data, but the database should remain correct without them.

### 6.6 Retrieval is a first-class feature

The project is not solved by "put embeddings in a vector database."

High-quality retrieval should support:

- lexical search;
- metadata filters;
- time filters;
- source filters;
- graph traversal;
- multiple queries;
- progressive expansion;
- exact source windows;
- token budgets;
- model-controlled research loops.

### 6.7 Explicit privacy boundaries

If the user chooses a temporary or private conversation mode in a provider, capture should be disabled by default.

A memory product should never undermine an explicit privacy action.

### 6.8 Inspectability

The user should be able to see:

- what was stored;
- where it came from;
- what was derived;
- why two things are linked;
- which items are curated;
- which items are raw;
- what a model retrieved.

## 7. Desired feeling

The GUI should not feel like database administration.

It should feel like a personal context playground:

- searchable history;
- living project map;
- graph exploration;
- drag-and-drop relationships;
- memory editing;
- source inspection;
- file ingestion;
- context previews;
- semantic zoom from large domains down to exact original messages.

The visual metaphor is closer to a "mind place" or "hive mind" than to a settings screen.

## 8. What Mind Place is not

At least initially, Mind Place is not:

- a new foundation model;
- a mandatory local LLM runtime;
- an autonomous agent that constantly rewrites personal data;
- a replacement for provider chat interfaces;
- a giant knowledge graph where every message is exposed as a visible node;
- a system that summarizes and deletes original conversations;
- a vector-database demo;
- a provider-specific memory clone.

It is infrastructure for continuity.
