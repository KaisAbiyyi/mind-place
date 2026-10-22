# MP-LIC-00 — MIT transition, copyright provenance, and release gate

Status: **proposed in a dedicated cloud PR; do not infer legal approval or a released MIT tag from this document.** Baseline: `main` commit `e9c1edd31a74318ba6b9266b01c56b7da3a8ee71` (22 October 2026), AGPL-3.0-or-later. Task: [#33](https://github.com/KaisAbiyyi/mind-place/issues/33).

## Provenance inventory

- **Git commits:** All 103 commits exposed by the GitHub repository history up to the baseline use the author identity `Kais Abiyyi` / `KaisAbiyyi` (two email spellings for the same displayed identity). This identifies recorded Git commit authors, **not legal ownership of all original or borrowed material**, work-made-for-hire status, or the identity of upstream rights holders.
- **Source and assets:** Repository tree has first-party Rust crates, Tauri app, web UI, browser extension, repository scripts, docs, tests and icon assets. It has no tracked `.gitmodules`, `node_modules`, `target`, binary installer, third-party source subtree or committed `THIRD-PARTY-NOTICES.txt`. The bespoke `src-tauri/icons/icon.svg` is source for the app icon variants; provenance of derived icons/assets still requires owner confirmation.
- **Influence versus copying:** [ACKNOWLEDGEMENTS.md](../../ACKNOWLEDGEMENTS.md) attributes architectural influence to Chat On Steroids and explicitly says its application source tree is not vendored here. That statement is not a line-by-line similarity or legal source audit.
- **Frontend lockfile:** `package-lock.json` at baseline includes 138 third-party package records. Declared SPDX/license counts (including dev packages): MIT 108, MIT-0 2, Apache-2.0 5, Apache-2.0 OR MIT 13, (MPL-2.0 OR Apache-2.0) 1, BSD-2-Clause 3, BSD-3-Clause 2, ISC 2, BlueOak-1.0.0 1, CC0-1.0 1; only the first-party root declared AGPL. No AGPL/GPL npm dependency declaration was found in this inventory. This is not a substituted audit of bundled artifacts.
- **Rust lockfile:** `Cargo.lock` contains 446 crate/version entries but does not itself declare all licenses. Windows CI runs `scripts/generate-notices.mjs` using target `cargo metadata` and emits third-party license texts into the package. **Review generated Windows third-party notices and dual-license selections before clearing this gate.** In particular, account for separately licensed dependencies and platform libraries.
- **Bundled binary:** Windows builds download a pinned, SHA-verified OpenAI `tunnel-client.exe` helper and stage upstream `tunnel/` notice files plus `THIRD-PARTY-NOTICES.txt`. The root MIT license never supersedes third-party notices.

## Blocking approval checklist

- [ ] Copyright owner confirms that all first-party source, original icon assets, documentation, tests, and any AI-assisted contributions in the current tree are within their right to relicense, or documents each permission/removal/rewrite.
- [ ] Resolve employer/client/university assignments (if applicable) and any outside contributions not covered by the repository owner's licensing authority; a commit author is not a legal rights certificate.
- [ ] Independently review the actual **Windows production** dependency/notice inventory and bundled helper's upstream license and obligations. Preserve mandatory attributions; flag any incompatible first-party code.
- [ ] Independent PR diff review, final-head Windows CI/installer/portable checks (including MIT vs third-party notices), and confirmation that no active first-party AGPL declaration remains.
- [ ] Only then merge and record the exact MIT-effective `main` commit, first MIT release tag and corresponding packaged-artifact notices. The owner makes the release decision.

**Rights attestation evidence:** Not yet filed. Attach a dated owner-authored statement or permissions evidence to [issue #33](https://github.com/KaisAbiyyi/mind-place/issues/33) before treating this task as cleared.

## Distribution rule

MIT applies only to rights-cleared Mind Place first-party material in revisions intentionally offered under the new license. Existing AGPL distributions keep the AGPL rights their recipients already obtained. Do not replace third-party licenses with MIT or retroactively relabel older tags/releases. A preexisting AGPL copy with a later permissive fork is still licensed according to the notices provided with that copy.

The bundled portable distribution must include matching first-party `LICENSE` and separate `THIRD-PARTY-NOTICES.txt`; Windows tunnel-helper notices remain under `tunnel/`. Refer to the release workflow, which validates portable assets after packaging.
