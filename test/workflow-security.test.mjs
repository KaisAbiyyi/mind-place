import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

test("GitHub Actions fork trust-boundary policy is enforced", () => {
  const result = spawnSync(
    process.execPath,
    ["scripts/verify-workflow-security.mjs"],
    { cwd: repositoryRoot, encoding: "utf8" },
  );

  assert.equal(
    result.status,
    0,
    `Workflow security policy failed.\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
  );
  assert.match(result.stdout, /PASS:/);
  assert.match(result.stdout, /Pending active-lock workflow files/);
});
