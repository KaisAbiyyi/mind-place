# MP-4E — bounded document extractors

Cloud implementation boundary: an independent Rust workspace crate, mind-place-documents.
MP-4A is the prerequisite; MP-4B must supply authorized, containment-checked handles
and MP-4D the versioned binary object before 4F wires extraction into indexing. This
crate deliberately never opens a path, consults a folder registry, writes extracted
objects, talks to MCP, or mutates the raw archive.

## API and invariants

- DocumentFormat::from_extension selects an allowlisted format (not a trust decision).
- extract_bytes(format, bytes, limits) is a pure helper for already-authorized bytes.
- extract_with_check(format, bytes, limits, check) accepts a synchronous authorization
  and cancellation callback. It runs on entry, between bounded loops and on exit.
  The caller **must** check the authority lease again before persisting/indexing
  any output. No partial results are returned on parser error, cancellation or
  resource exhaustion.
- ExtractedDocument has deterministic text, ordered sections/chunks and public
  capability warnings. A section label is **not** a verified file/page identity.
  Indexers must attach MP-4A/4D authoritative source identity/version externally.
- Default caps: 16 MiB input, 32 MiB aggregate expanded content, 2 MiB text
  output, 1024 chunks (4 KiB each), 512 ZIP entries, 10,000 PDF objects/content
  streams, 250 PDF page markers, XML depth 96 and cooperative 2000 ms deadline.
  Every cap is configurable, but zero caps are rejected. No recursive unpacking.
- ZIP scans the end-of-central-directory and all central records before expanding,
  rejects ZIP64, encrypted entries, duplicate names, unsafe entry paths, malformed
  lengths, damaged CRC and unbounded declared uncompressed sizes. Only allowlisted
  Word, PowerPoint and Excel XML parts are decompressed, directly into bounded
  memory. Nothing is extracted to disk. External relationships are not followed.
- XML parsing rejects DTD/general entity references; only explicit text/cell nodes
  contribute user-visible text. OOXML document structure is divided by part.
- UTF-8 text/source/Markdown reject NUL/invalid encoding. Markdown headings act
  as section hints; large sections split into bounded Unicode-safe chunks.
- PDF uses conservative literal/hex text parsing on plain/Flate streams. It does
  **not** implement all PDF object streams, font ToUnicode CMaps, encryption,
  embedded media, OCR, tagged structure, true page association or layout. Never
  treat it as complete text for arbitrary PDFs. Known-unsupported compressed
  streams are explicitly warned about instead of guessed.

## Threat cases and scope

Unit fixtures cover text format selection, Markdown headings, source text, DOCX,
PPTX, XLSX, deflate entries, CRC damage, central-directory duplicate/traversal,
declared ZIP expansion bomb, DTD, XML-depth exhaustion, input/output limits,
revocation and simple PDF text streams/page bounds.

The resource deadline is **cooperative**; it is not a hardened OS process CPU or
memory sandbox. A host-owned bounded worker/process and operating-system watchdog
are still required before handling arbitrarily hostile files in production.
This module does not authorize filesystem reads, satisfy Windows/NTFS gate MP-4L1,
or qualify representative document corpus gate MP-4L2. 4F is responsible for
source/version provenance, disclosable search results, revalidation and UI.

## Future integration contract

1. 4B acquires and rechecks an approved-root contained file handle; 4D persists
   immutable binary with content hash and version.
2. 4F resolves DocumentFormat from the approved name, enforces file-size caps,
   reads through the authorized handle, then extracts with lease callback.
3. 4F rechecks epoch/root/version authorization *after* parsing and again before
   index transaction commit or result disclosure. A revoked read discards output.
4. Record extractor revision and warnings alongside derived chunks. Never rewrite
   original binary content or treat missing text as proof that no source content
   exists.

Claim: MP-4E, ChatGPT cloud session 20261021-chatgpt-mp4e-001,
exclusive resource document-extractors.
