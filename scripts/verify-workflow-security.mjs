import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workflowDirectory = resolve(repositoryRoot, ".github", "workflows");

const pendingActiveLocks = new Set([
  "ci.yml",
  "mp-10c-scale.yml",
  "storage-qualification.yml",
]);

const fullyHardened = new Set([
  "mp10b-bakeoff.yml",
  "mp8f-visual.yml",
  "mp9a-security-probe.yml",
  "release.yml",
  "workflow-security.yml",
]);

const expectedFiles = [...pendingActiveLocks, ...fullyHardened].sort();
const actualFiles = readdirSync(workflowDirectory)
  .filter((file) => /\.ya?ml$/i.test(file))
  .sort();

assert.deepEqual(
  actualFiles,
  expectedFiles,
  "Workflow inventory changed. Audit the new/removed workflow and update this explicit inventory before proceeding.",
);

for (const file of actualFiles) {
  const source = readFileSync(resolve(workflowDirectory, file), "utf8");

  assert.match(source, /^permissions:\s*$/m, `${file}: explicit workflow permissions are required`);
  assert.match(source, /^  contents:\s*read\s*$/m, `${file}: contents must remain read-only`);

  assert.doesNotMatch(
    source,
    /^\s*(?:pull_request_target|workflow_run|issue_comment|workflow_call)\s*:/m,
    `${file}: privileged or reusable event requires a separate reviewed trust design`,
  );
  assert.doesNotMatch(source, /\$\{\{\s*secrets\./, `${file}: target-repository workflows must not reference secrets`);
  assert.doesNotMatch(source, /^\s*permissions:\s*write-all\s*$/m, `${file}: write-all token permissions are forbidden`);
  assert.doesNotMatch(
    source,
    /^\s*(?:contents|actions|checks|pull-requests|packages|statuses|deployments|id-token):\s*write\s*$/m,
    `${file}: write-scoped token permissions are forbidden in this CI trust boundary`,
  );

  if (pendingActiveLocks.has(file)) {
    continue;
  }

  const actionRefs = [...source.matchAll(/^\s*uses:\s*([^\s#]+)(?:\s+#.*)?$/gm)].map((match) => match[1]);
  assert.ok(actionRefs.length > 0, `${file}: expected at least one action reference`);

  for (const actionRef of actionRefs) {
    if (actionRef.startsWith("./")) continue;
    assert.match(
      actionRef,
      /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+@[0-9a-f]{40}$/,
      `${file}: external action must be pinned to a full commit SHA: ${actionRef}`,
    );
  }

  const lines = source.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const checkout = lines[index].match(/^(\s*)-\s+uses:\s*actions\/checkout@/);
    if (!checkout) continue;

    const stepIndent = checkout[1].length;
    const block = [];
    for (let next = index + 1; next < lines.length; next += 1) {
      const nextLine = lines[next];
      const nextIndent = nextLine.match(/^\s*/)[0].length;
      if (/^\s*-\s/.test(nextLine) && nextIndent === stepIndent) break;
      if (nextLine.trim() && nextIndent < stepIndent) break;
      block.push(nextLine);
    }

    assert.match(
      block.join("\n"),
      /^\s+persist-credentials:\s*false\s*$/m,
      `${file}: actions/checkout must not persist its token in local Git configuration`,
    );
  }
}

console.log(
  `PASS: ${fullyHardened.size} workflows use immutable action SHAs and non-persistent checkout; all ${actualFiles.length} workflows enforce the read-only/no-secret/no-privileged-trigger boundary. Pending active-lock workflow files: ${[...pendingActiveLocks].join(", ")}.`,
);
