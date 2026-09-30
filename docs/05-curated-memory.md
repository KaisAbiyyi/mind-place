# Curated Memory Model

## 1. Raw history is not the same as memory

Mind Place uses the word memory in two distinct senses:

1. The broad archive of everything that happened.
2. The compact set of facts, preferences, and decisions deliberately kept as long-term context.

This document refers to the second as curated memory.

Raw conversation solves recall.

Curated memory solves immediate continuity.

## 2. Who decides what becomes curated memory?

The preferred design is:

> The active AI model or the user decides.

The local storage engine should not need an internal AI classifier that constantly asks whether every sentence matters.

This mirrors the useful behavior of capable assistants:

- the model hears new information;
- it judges whether the information is likely to matter later;
- it decides the appropriate abstraction;
- it chooses whether to create or update memory.

Mind Place supplies durable CRUD and search primitives.

## 3. Minimal memory representation

V1 can remain deliberately simple.

Conceptually:

~~~json
{
  "id": "mem_...",
  "content": "User prefers a specific model for a specific implementation workflow.",
  "type": "preference",
  "topic": "AI workflow",
  "created_at": "...",
  "updated_at": "..."
}
~~~

The system should avoid prematurely creating an enormous ontology.

Useful optional fields later:

- status;
- importance;
- valid_from;
- valid_until;
- supersedes;
- source;
- project;
- confidence;
- author or model.

## 4. Search before write

A model should normally check for an existing related memory before creating another one.

Preferred flow:

~~~text
new durable information
      |
search related memory
      |
existing memory?
  |             |
 yes            no
  |             |
update       create
~~~

This prevents duplication and allows preferences to evolve.

## 5. History and supersession

Do not assume the latest truth requires deleting the past.

Example:

~~~text
Earlier:
Preferred implementation model: Model A

Later:
Preferred implementation model: Model B
~~~

Possible representation:

~~~text
memory A
valid_until: later date
superseded_by: memory B

memory B
valid_from: later date
~~~

This preserves historical context.

The exact temporal model can be deferred until needed, but the schema should avoid making it impossible.

## 6. Provenance

A curated memory should ideally point back to supporting raw sources.

~~~text
Curated memory
"Prefer immediate-execution wording in implementation prompts."
       |
       +--> conversation C
              |
              +--> message M
~~~

This enables:

- source inspection;
- debugging incorrect memory;
- user trust;
- later re-curation using better models;
- conflict resolution.

## 7. Manual memories

The user should be able to create a memory directly without a conversation.

Examples:

- type a note;
- drag a message into the memory area;
- select text from a file;
- create a project rule;
- pin a preference.

Manual content should be a first-class source, not treated as lower quality than model-generated memories.

## 8. Memory types

A lightweight initial set could be:

- fact;
- preference;
- project;
- decision;
- workflow;
- relationship;
- constraint;
- note.

Types should remain extensible.

It may be better to begin with a string type column than a strict enum.

## 9. Memory scope

Useful scopes may emerge:

- global;
- project;
- person;
- repository;
- provider;
- task or workflow.

A rule such as "use Conventional Commits" may be global or repository-specific.

The model can use scope filters during retrieval.

## 10. Forgetting and deletion

The user must retain final control.

Operations should include:

- edit;
- supersede;
- archive;
- delete;
- detach incorrect source;
- merge duplicates.

Deletion policy should distinguish:

- curated memory deletion;
- raw source deletion;
- derived graph or index deletion.

Deleting a curated memory should not automatically destroy its raw source conversation unless explicitly requested.

## 11. Why not auto-curate everything?

Automatic curation has risks:

- false inferences;
- overgeneralization;
- duplicated preferences;
- turning jokes or temporary thoughts into permanent facts;
- unnecessary computational cost;
- dependence on a particular model.

Mind Place can support optional background curation later, but it should not be necessary for the product to work.

## 12. Future assistant behavior

An ideal model connected to Mind Place should naturally do this:

~~~text
User:
From now on use X for implementations.

Model:
1. recognizes a durable preference;
2. searches for existing implementation-model preference;
3. updates or supersedes it;
4. continues the conversation.
~~~

For irrelevant short-lived information, it simply does not create curated memory.

This is where model intelligence is valuable: not inside the database, but in deciding how to use it.
