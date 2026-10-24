# MP-10C — Cloud document scale, resource, reconciliation and restart gates

**Prerequisites:** MP-10A and MP-4F merged. **Lock:** `benchmark-scale`. **Environment:** GitHub Windows cloud only.

## Real synthetic workload

The ignored Rust test `crates/index-core/tests/document_scale_qualification.rs` creates **physical synthetic Markdown documents** in OS-scoped disposable temporary storage; explicitly registers and approves each root in real `StateStore`; invokes production `DocumentIndex::refresh_root` (NTFS scanner, immutable object versions, bounded extractor, FTS5), evidence-authenticated search, reindex/reconciliation, and `StateStore` / `DocumentIndex` reopen. No user's files, archive, accounts, or existing database is accessed.

The two exact tiers are **100 files/1,000 actual chunks** (smoke) and **100,000 files/1,000,000 actual chunks** (qualification). Every Markdown file has ten distinct headings which the real parser turns into ten sections/chunks. Production caps at **10,000 files per approved root**, so the full tier uses ten separately approved roots; security caps are not raised. The production storage qualification already covers 1M archive message chunks separately; its 100 conversations are not 100K documents.

Hard correctness gates: exact number of files and extracted chunks, no skipped files, authoritative live-checked first/last rare evidence, same ranked hits after SQLite reopen, authenticated root reconciliation, integrity check, FTS consistency check, and unchanged SHA-256 of every fixture file. Failure fails the job. JSON receipts report generation/index/reconciliation and database reopen wall-clock timing, warm query median, Windows test process peak working set, derived/authority disk bytes, bundled SQLite, counts and full-input hash.

## Cloud verification

The PR-triggered `.github/workflows/mp-10c-scale.yml` executes both Windows tiers as distinct jobs (180-minute timeout), uploads separate machine-readable receipts, and checks Rust format. Run locally only on an isolated test machine using `MP_10C_FILES=100` or `100000` plus `cargo test --release -p mind-place-index --test document_scale_qualification qualifies_document_scale -- --ignored --exact --nocapture`.

**Acceptance requires** exact-HEAD Windows product CI, **both** MP-10C document jobs successful with receipts, exact-head implementor self-review, and no broken security or integrity invariants. A passing 100-file smoke test alone does **not** qualify 100K. Missing, timed-out or failing jobs are **NOT PASS** and the task stays **Review** until fixed/qualified.

**Honest scope:** timings are hosted-runner observations, not calibrated SLAs; peak working set refers to the benchmark process, not incremental app memory. SQLite reopen is not packaged Tauri/WebView2 cold startup. Single-root reconciliation is not all-root/background watcher latency. User hardware, real documents, native desktop launch, UI responsiveness and 10L1 remain **NOT_RUN**. No MP-10A gold labels are modified.