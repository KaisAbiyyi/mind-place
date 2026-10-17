# Acknowledgements

Mind Place is an independent open-source project, but it benefits from ideas and engineering patterns developed across the local AI tooling community.

## Chat On Steroids

[Chat On Steroids](https://github.com/totec448-spec/chat-on-steroids) is explicitly credited as a major architectural influence on Mind Place.

In particular, Mind Place's direction for local AI integration is informed by Chat On Steroids' work around:

- user-approved filesystem roots as an explicit permission boundary;
- canonical path validation and protection against traversal, symlink, junction, and reparse-point escapes;
- a small MCP tool surface with bounded results rather than an unbounded collection of specialized tools;
- local-first session data and durable state;
- HTTPS tunnel connectivity for hosted ChatGPT MCP clients;
- capability-scoped connectors and runtime permission checks;
- keeping project association separate from filesystem permission.

Mind Place applies those lessons to a different product goal: durable personal continuity, immutable conversation evidence, personal documents, retrieval, curated memory, and knowledge graphs that remain independent of any one AI provider.

The current Mind Place repository does not vendor or embed the Chat On Steroids application source tree. If compatible third-party source is incorporated in the future, its original copyright and license notices must be retained in the appropriate third-party notices.

Chat On Steroids is licensed separately by its own authors and contributors. Mind Place is not affiliated with or endorsed by Chat On Steroids, OpenAI, Anthropic, Google, or other model providers.
