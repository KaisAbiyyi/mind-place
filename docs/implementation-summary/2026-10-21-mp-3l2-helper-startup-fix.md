# MP-3L2 helper startup failure — cloud fix and local retest

**Date:** 2026-10-21. **Gate:** MP-3L2 remains LOCAL/PARTIAL pending actual OpenAI hosted access.

## Trigger and diagnosis

The installed Mind Place 0.2.5 exposes local MCP on `127.0.0.1:43731` but its official adjacent `tunnel-client.exe` exits immediately after the owner's `Start secure tunnel` action. The UI initially reports helper process started, then reports Stopped on status refresh. No tunnel request was proven.

The pinned official tunnel-client version `0.0.16` was checked against the installed executable's SHA-256 pin. Running the exact Mind Place startup CLI with **dummy** tunnel ID and dummy runtime credentials reproduces this fatal configuration error:

```text
configure tunnel-client: log level requires 'struct-text' or 'json' log format
```

The helper invocation had `--log.level=warn` but lacked `--log.format`. Adding `--log.format=struct-text` kept the same dummy-key process running beyond the 4.5-second probe window, without the fatal error. The local `doctor --json` preflight passed all configuration checks with the dummy values. Private owner keys and archive contents were neither read nor stored.

## Cloud fix

Set an explicit supported helper log format in `src-tauri/src/hosted_tunnel.rs`. Preserve the existing isolated child environment, null stdout/stderr, ephemeral read credential, pinned binary integrity verification, loopback-only origin and fail-closed lifecycle. Extend the existing command-argument test to prevent this exact startup regression.

This fix establishes **helper startup argument validity only**, not end-to-end hosted connectivity or ChatGPT acceptance. The original remote service permissions and account access still need live verification.

## Windows retest after CI and integration

1. Install the new Windows package built from the merged fix SHA; keep local user archives and keys private.
2. Unlock Mind Place, start local MCP and enter the existing tunnel ID/runtime key **only inside the app**, consent to remote access, and start the secure tunnel.
3. Check the Mind Place-owned helper process remains alive and that an authenticated remote client can discover only `memory_search`, `memory_fetch`, `context_build` and complete read-only calls.
4. Check stop/restart/revocation/Lock denies the next hosted request.
5. Save a sanitized `PASS/FAIL/PARTIAL/BLOCKED` receipt and update [MP-3L2 issue #19](https://github.com/KaisAbiyyi/mind-place/issues/19) and Notion. Do not mark MP-3L2/3U complete based only on GitHub CI or helper liveness.
