import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
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
  assert.match(result.stdout, /zero active-lock exceptions/);
});

function checkMutatedWorkflow(t, filename, mutate) {
  const root = mkdtempSync(join(tmpdir(), "mind-place-workflow-policy-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, "scripts"));
  mkdirSync(join(root, ".github"));
  cpSync(resolve(repositoryRoot, ".github/workflows"), join(root, ".github/workflows"), { recursive: true });
  cpSync(resolve(repositoryRoot, "scripts/verify-workflow-security.mjs"), join(root, "scripts/verify-workflow-security.mjs"));
  const path = join(root, ".github/workflows", filename);
  const original = readFileSync(path, "utf8");
  const changed = mutate(original);
  assert.notEqual(changed, original, "The adverse fixture must actually change the workflow");
  writeFileSync(path, changed);
  return spawnSync(process.execPath, [join(root, "scripts/verify-workflow-security.mjs")], { encoding: "utf8" });
}

for (const file of ["ci.yml", "mp-10c-scale.yml", "storage-qualification.yml", "mp9b-sqlcipher-qualification.yml", "windows-product.yml", "brave-registration.yml"]) {
  test(`${file}: mutable checkout references cannot bypass the workflow audit`, (t) => {
    const result = checkMutatedWorkflow(t, file, (source) => source.replace(/actions\/checkout@[0-9a-f]{40}/, "actions/checkout@v5"));
    assert.notEqual(result.status, 0, "A mutable checkout tag must fail even on an unnamed '- uses' step");
    assert.match(result.stderr, /external action must be pinned/);
  });
}

test("foundation checkout credentials cannot be persisted", (t) => {
  const result = checkMutatedWorkflow(t, "mp9b-sqlcipher-qualification.yml", (source) => source.replace("persist-credentials: false", "persist-credentials: true"));
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /must not persist its token/);
});


test("BRAVE-REG-01 checkout credentials cannot be persisted", (t) => {
  const result = checkMutatedWorkflow(t, "brave-registration.yml", (source) => source.replace("persist-credentials: false", "persist-credentials: true"));
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /must not persist its token/);
});
