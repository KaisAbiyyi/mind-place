CREATE TABLE index_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

INSERT INTO index_meta(key, value) VALUES('generation', '0');

CREATE TABLE revisions (
    source_id TEXT NOT NULL,
    archive_id TEXT NOT NULL,
    snapshot_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    title TEXT NOT NULL,
    source_url TEXT NOT NULL,
    captured_at TEXT NOT NULL,
    completeness TEXT NOT NULL,
    is_current INTEGER NOT NULL CHECK(is_current IN (0, 1)),
    seen_generation INTEGER NOT NULL,
    PRIMARY KEY(source_id, snapshot_id)
) WITHOUT ROWID;

CREATE INDEX revisions_archive ON revisions(archive_id);
CREATE INDEX revisions_current ON revisions(is_current, provider);

CREATE TABLE chunks (
    id INTEGER PRIMARY KEY,
    source_id TEXT NOT NULL,
    snapshot_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    part INTEGER NOT NULL,
    kind TEXT NOT NULL,
    role TEXT NOT NULL,
    model TEXT,
    body TEXT NOT NULL,
    UNIQUE(source_id, snapshot_id, kind, message_id, part),
    FOREIGN KEY(source_id, snapshot_id)
        REFERENCES revisions(source_id, snapshot_id) ON DELETE CASCADE
);

CREATE INDEX chunks_provenance ON chunks(source_id, snapshot_id, message_id, part);

-- Keep message text only once; the FTS b-tree references chunks row IDs.
CREATE VIRTUAL TABLE chunks_fts USING fts5(
    body, content='chunks', content_rowid='id', tokenize='unicode61'
);

CREATE TRIGGER chunks_ai AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts(rowid, body) VALUES(new.id, new.body);
END;

CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, body)
    VALUES('delete', old.id, old.body);
END;

CREATE TRIGGER chunks_au AFTER UPDATE OF body ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, body)
    VALUES('delete', old.id, old.body);
    INSERT INTO chunks_fts(rowid, body) VALUES(new.id, new.body);
END;
