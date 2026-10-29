//! Phase 2D: ephemeral, local-only inspection of retrieval operations.
//! Never persist raw prompts, snippets, document text, source URLs, or PINs
//! in a diagnostic trace. Evidence payload is separately unlock protected.
use crate::{
    CaptureArchiveReader, ContextPack, ContextRequest, FetchRequest, FetchResponse, IndexStore,
    Result, RetrievalRequest, RetrievalResponse,
};
use mind_place_archive::ArchiveStore;
use serde::Serialize;
use std::time::Instant;

/// Bounded metadata for one call. No query strings, content, URLs or source IDs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RetrievalTrace {
    pub schema_version: u32,
    pub operation: &'static str,
    /// Monotonic observed elapsed duration. Not an OS-level deadline.
    pub elapsed_us: u128,
    pub index_generation: i64,
    pub candidate_count: usize,
    pub result_count: usize,
    pub source_count: usize,
    pub text_bytes: usize,
    pub estimated_tokens: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchInspection {
    pub trace: RetrievalTrace,
    /// Contains evidence snippets and full ranked provenance, including the
    /// per-query BM25 components. Must be treated like a protected search call.
    pub result: RetrievalResponse,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ContextInspection {
    pub trace: RetrievalTrace,
    pub result: ContextPack,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FetchInspection {
    pub trace: RetrievalTrace,
    pub result: FetchResponse,
}

impl IndexStore {
    /// Read the generation directly instead of calling stats(), whose full
    /// COUNT(DISTINCT) / COUNT(*) scans scale with a million derived rows.
    fn inspect_generation(&self) -> Result<i64> {
        let db = self.connection()?;
        Ok(db.query_row(
            "SELECT CAST(value AS INTEGER) FROM index_meta WHERE key='generation'",
            [],
            |row| row.get(0),
        )?)
    }

    /// Local diagnostic surface for the real Phase 2A pipeline, not a replay.
    pub fn inspect_retrieval_with_check(
        &self,
        request: &RetrievalRequest,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<SearchInspection> {
        check()?;
        let started = Instant::now();
        let result = self.retrieve(request)?;
        check()?;
        let generation = self.inspect_generation()?;
        check()?;
        let mut sources = std::collections::BTreeSet::new();
        for hit in &result.hits {
            sources.insert(&hit.source_id);
        }
        Ok(SearchInspection {
            trace: RetrievalTrace {
                schema_version: 1,
                operation: "search",
                elapsed_us: started.elapsed().as_micros(),
                index_generation: generation,
                candidate_count: result.candidate_count,
                result_count: result.hits.len(),
                source_count: sources.len(),
                text_bytes: 0, // snippet JSON bytes are NOT a fetched-context budget.
                estimated_tokens: 0,
                truncated: result.truncated,
            },
            result,
        })
    }

    pub fn inspect_retrieval(&self, request: &RetrievalRequest) -> Result<SearchInspection> {
        self.inspect_retrieval_with_check(request, || Ok(()))
    }

    pub fn inspect_context_with_check(
        &self,
        archive: &ArchiveStore,
        request: &ContextRequest,
        check: impl FnMut() -> Result<()>,
    ) -> Result<ContextInspection> {
        self.inspect_context_generic_with_check(archive, request, check)
    }

    #[cfg(feature = "sqlcipher-preview")]
    pub fn inspect_context_vault_with_check(
        &self,
        archive: &crate::vault_archive::VaultCaptureStore,
        request: &ContextRequest,
        check: impl FnMut() -> Result<()>,
    ) -> Result<ContextInspection> {
        self.inspect_context_generic_with_check(archive, request, check)
    }

    fn inspect_context_generic_with_check(
        &self,
        archive: &impl CaptureArchiveReader,
        request: &ContextRequest,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<ContextInspection> {
        check()?;
        let started = Instant::now();
        let result = self.build_context_generic_with_check(archive, request, &mut check)?;
        check()?;
        let generation = self.inspect_generation()?;
        check()?;
        Ok(ContextInspection {
            trace: RetrievalTrace {
                schema_version: 1,
                operation: "context",
                elapsed_us: started.elapsed().as_micros(),
                index_generation: generation,
                candidate_count: result.candidate_count,
                result_count: result.excerpt_count,
                source_count: result.source_count,
                text_bytes: result.text_bytes,
                estimated_tokens: result.estimated_tokens,
                truncated: result.truncated,
            },
            result,
        })
    }

    pub fn inspect_context(
        &self,
        archive: &ArchiveStore,
        request: &ContextRequest,
    ) -> Result<ContextInspection> {
        self.inspect_context_with_check(archive, request, || Ok(()))
    }

    pub fn inspect_fetch_with_check(
        &self,
        archive: &ArchiveStore,
        request: &FetchRequest,
        check: impl FnMut() -> Result<()>,
    ) -> Result<FetchInspection> {
        self.inspect_fetch_generic_with_check(archive, request, check)
    }

    #[cfg(feature = "sqlcipher-preview")]
    pub fn inspect_fetch_vault_with_check(
        &self,
        archive: &crate::vault_archive::VaultCaptureStore,
        request: &FetchRequest,
        check: impl FnMut() -> Result<()>,
    ) -> Result<FetchInspection> {
        self.inspect_fetch_generic_with_check(archive, request, check)
    }

    fn inspect_fetch_generic_with_check(
        &self,
        archive: &impl CaptureArchiveReader,
        request: &FetchRequest,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<FetchInspection> {
        check()?;
        let started = Instant::now();
        let result = self.fetch_generic_with_check(archive, request, &mut check)?;
        check()?;
        let generation = self.inspect_generation()?;
        check()?;
        Ok(FetchInspection {
            trace: RetrievalTrace {
                schema_version: 1,
                operation: "fetch",
                elapsed_us: started.elapsed().as_micros(),
                index_generation: generation,
                candidate_count: 0, // Direct pinned source fetch does not search.
                result_count: result.segments.len(),
                source_count: 1,
                text_bytes: result.text_bytes,
                estimated_tokens: result.estimated_tokens,
                truncated: result.truncated,
            },
            result,
        })
    }

    pub fn inspect_fetch(
        &self,
        archive: &ArchiveStore,
        request: &FetchRequest,
    ) -> Result<FetchInspection> {
        self.inspect_fetch_with_check(archive, request, || Ok(()))
    }
}
