# Retrieval and Context Engine

## 1. Retrieval is the central engineering problem

Storing everything is easy.

Giving a model the right 5,000 to 20,000 tokens from years of personal history is difficult.

Therefore Mind Place should optimize the fetch path, not aggressively shrink the archive.

The project should treat retrieval quality as a first-class system, comparable in importance to the capture engine.

## 2. Avoid the simplistic RAG model

A naive memory system does:

~~~text
user query
   |
embed query
   |
top 5 nearest chunks
   |
inject into prompt
~~~

That is insufficient for complex personal context.

Questions may require:

- multiple topics;
- chronology;
- obscure lexical matches;
- project relationships;
- exact quotes;
- information across several conversations;
- a current decision plus historical reason;
- a file plus a conversation.

Mind Place should allow the model to perform iterative research.

## 3. Research-style retrieval

Preferred pattern:

~~~text
USER QUESTION
      |
      v
model decomposes information need
      |
      +---------+---------+
      |         |         |
   search A  search B  search C
      |         |         |
      +---------+---------+
                |
         inspect snippets
                |
       identify missing context
                |
        second retrieval wave
                |
         graph traversal
                |
       exact source windows
                |
        compact context pack
~~~

This resembles research modes in modern assistants more than conventional single-pass memory lookup.

## 4. Few primitives, powerful configuration

Instead of hundreds of narrowly named tools, provide a small set with rich parameters.

Illustrative search shape:

~~~json
{
  "queries": [
    "project launch strategy",
    "generative AI decision",
    "low cost feature"
  ],
  "sources": ["raw", "curated", "files"],
  "providers": ["chatgpt", "claude"],
  "mode": "hybrid",
  "time": {
    "from": "2026-09-01",
    "to": "2026-09-30"
  },
  "graph": {
    "expand": true,
    "depth": 2
  },
  "granularity": "chunk",
  "group_by": "conversation",
  "max_results": 50,
  "token_budget": 12000
}
~~~

The server remains deterministic. The model decides how to configure the search.

## 5. Progressive fetch

Search results should initially be cheap.

First response:

- conversation ID;
- title;
- date;
- provider;
- compact snippet;
- matching terms;
- related memory IDs;
- relevance or ranking metadata.

The model can then request a local window around the matching message.

Then it broadens only if necessary.

Full conversation retrieval should be a deliberate last step.

This avoids dumping a 500K-token conversation into context because one matching sentence was needed.

## 6. Retrieval granularity

Potential granularities:

- metadata only;
- search snippets;
- message;
- message window;
- chunk;
- section;
- conversation summary;
- full conversation;
- file excerpt;
- full file;
- graph neighborhood;
- curated-memory bundle.

The model should be able to choose the cheapest sufficient level.

## 7. Token budget as an explicit contract

A powerful design is to make context cost explicit.

Example request:

~~~json
{
  "token_budget": 16000
}
~~~

The context builder might return:

~~~text
8 curated memories       about 1,200 tokens
12 source excerpts       about 7,400 tokens
3 graph neighborhoods    about 2,100 tokens
4 file excerpts          about 4,300 tokens
---------------------------------------------
total                    about 15,000 tokens
~~~

This makes retrieval predictable and prevents memory from consuming an entire model context window.

## 8. Search modes

### 8.1 Lexical

SQLite FTS5 and BM25.

Excellent for:

- names;
- project identifiers;
- error messages;
- branch names;
- issue numbers;
- model names;
- uncommon phrases;
- exact terminology.

### 8.2 Structured

Filter by:

- date;
- provider;
- model;
- project;
- memory type;
- file type;
- conversation;
- person or entity;
- source type.

### 8.3 Graph

Traverse known relationships.

Graph expansion can discover relevant material that did not share exact words with the prompt.

### 8.4 Semantic

Optional vectors later.

Useful when the user asks conceptually similar things with unrelated wording.

Should not be mandatory for V1.

### 8.5 Hybrid

Combine lexical, structured, graph, and eventually vector signals.

## 9. Ranking

A future hybrid score may combine:

- FTS relevance;
- semantic similarity;
- recency;
- memory priority;
- source authority;
- graph distance;
- user pinning;
- exact entity match;
- conversation activity.

Do not encode complex ranking before collecting real failures.

## 10. Bulk fetching

The model should be able to request several searches in one round trip.

Benefits:

- lower tool-call latency;
- research-style decomposition;
- comparison across topics;
- efficient use of agent loops.

Example:

~~~text
query A: latest project state
query B: original design rationale
query C: previous failed approach
query D: user's current workflow preference
~~~

The server can execute those searches independently and return grouped results.

## 11. Context packs

A context pack is a derived, bounded bundle for a particular task.

Example:

~~~text
Context Pack: Continue project architecture

- current project definition
- architecture decisions
- unresolved questions
- recent conversation excerpts
- relevant files
- token estimate
- provenance references
~~~

Context packs can be ephemeral.

Caching may be useful when the same project context is repeatedly sent to a model.

## 12. Source fidelity

Search results should preserve a path back to exact source material.

A summary may say:

"Temporary chats should not be captured."

The model should be able to request the exact conversation excerpt where that decision was made.

This is important for disagreements, nuance, and future re-interpretation.

## 13. Retrieval observability

The GUI should eventually expose what happened during retrieval:

- queries issued;
- filters used;
- results selected;
- source windows fetched;
- graph hops;
- tokens returned.

This creates a personal research trace and helps improve retrieval quality.

## 14. Success metric

The retrieval engine is successful when a new model can enter a conversation with minimal prior context and reliably recover the right personal or project state without the user manually retelling it.

That matters more than maximizing a generic embedding benchmark.
