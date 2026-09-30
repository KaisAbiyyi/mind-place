# Knowledge Graph and Interactive UI

## 1. UI goal

Mind Place should make years of context explorable without turning the application into a database browser.

The desired experience is a hybrid of:

- personal knowledge map;
- conversation library;
- file explorer;
- memory editor;
- graph playground;
- retrieval debugger.

The visual identity can be sophisticated and modern, but performance and clarity matter more than decorative complexity.

## 2. The graph should be semantic

A dangerous approach is to render every raw message as a graph node.

A long-term archive could easily contain millions of messages.

A million-node visible graph is not useful; it is visual noise.

Instead, the default visible graph should represent semantic objects such as:

- projects;
- domains;
- topics;
- people and entities;
- important decisions;
- files;
- curated memories;
- selected conversations.

Raw messages remain reachable through drill-down.

## 3. Semantic zoom

The graph should support conceptual levels.

~~~text
Hive
 |
Domain
 |
Project / Topic
 |
Memory / Decision / File
 |
Conversation
 |
Raw message
~~~

At the top level, the user sees broad clusters.

Zooming or opening a cluster reveals finer structure.

This prevents the graph from becoming a bowl of noodles.

## 4. Example graph

~~~text
                       USER
                        |
          +-------------+-------------+
          |                           |
      PROJECT A                   PROJECT B
          |                           |
   +------+------+             +------+------+
   |             |             |             |
Analytics    AI workflow    File format   Benchmark
                 |
          +------+------+
          |             |
       Model A        Model B
~~~

This is conceptual, not a prescribed ontology.

## 5. Provenance interaction

Clicking a curated memory should show:

- memory text;
- type and scope;
- creation time;
- author or model;
- source conversations and files;
- superseded history;
- connected graph nodes.

Then the user can jump to the exact original message.

The graph should never obscure where information came from.

## 6. Direct manipulation

The playground idea suggests interactions such as:

- drag one node onto another to propose a relationship;
- create a topic cluster;
- pin an important node;
- merge duplicate semantic nodes;
- split an incorrect cluster;
- promote raw message to curated memory;
- connect a file to a project;
- remove an inferred edge;
- mark a relationship as manual or authoritative.

Manual edits should take precedence over inferred graph structure unless explicitly changed.

## 7. Potential primary layout

~~~text
+----------------+--------------------------------------+
| Library        |                                      |
|                |             HIVE GRAPH               |
| Conversations  |                                      |
| Memories       |          o------o                    |
| Files          |         / \    / \                   |
| Projects       |        o   o--o   o                  |
| People         |                                      |
| Topics         |                                      |
+----------------+--------------------------------------+
| Search / Context Playground / Retrieval Inspector     |
+-------------------------------------------------------+
~~~

The final design may differ, but the app should support both navigation and exploration.

## 8. Library views

### Conversations

Filters:

- provider;
- date;
- model;
- project or topic;
- captured or imported;
- branch;
- attachments.

### Memories

Filters:

- type;
- scope;
- status;
- source;
- created by;
- superseded or current.

### Files

Show:

- file metadata;
- hash and deduplication;
- extraction/index status;
- related projects;
- source conversations.

### Projects and topics

Aggregate:

- curated memories;
- recent conversations;
- files;
- graph neighbors;
- timeline.

## 9. Search as a playground

Search should expose more than one text box.

Advanced mode can let the user experiment with:

- lexical versus hybrid retrieval;
- time ranges;
- graph depth;
- result granularity;
- providers;
- token budget;
- source types.

This makes the retrieval engine understandable and tunable.

## 10. Context preview

Before sending context to a model, Mind Place can show:

~~~text
Context pack
- 9 memories
- 6 source excerpts
- 2 files
- about 11,800 tokens
~~~

The user may inspect or remove items.

This can be especially useful when testing the system.

## 11. Raw and derived visual language

The UI should clearly distinguish:

- raw source;
- user-authored content;
- model-authored curated memory;
- inferred relationship;
- cached summary.

This could use badges, icons, or typography rather than relying only on color.

Users should never confuse an inferred edge with a source fact.

## 12. Performance

A graph UI can become expensive.

Design constraints:

- never load all raw records into the graph;
- aggregate at broad zoom levels;
- query neighborhoods on demand;
- virtualize large lists;
- cap visible nodes;
- use incremental layout;
- cache stable node positions;
- avoid requiring GPU-heavy effects for normal use.

"Cool looking" and "lightweight" are both product requirements.

## 13. Desktop direction

A future Tauri shell is attractive because Mind Place is local infrastructure and may need:

- background process management;
- system tray controls;
- filesystem ingestion;
- browser-extension communication;
- local database access;
- notifications;
- native file picker;
- secure credential storage.

However, the core UI can begin as a local web application if that accelerates iteration.

## 14. Capture controls

The UI should always make capture state understandable.

Potential global controls:

- Capture ON or OFF;
- Pause all;
- Provider status;
- Queue status;
- Temporary-chat policy;
- Last captured message;
- Local server health.

Trust depends on predictability.
