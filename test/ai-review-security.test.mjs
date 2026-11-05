import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';

const policyUrl = new URL('../docs/AI-REVIEW-SECURITY.md', import.meta.url);
const manifestUrl = new URL('./fixtures/ai-review/manifest.json', import.meta.url);
const policy = await readFile(policyUrl, 'utf8');
const manifest = JSON.parse(await readFile(manifestUrl, 'utf8'));
const fixtureUrl = (path) => new URL('./fixtures/ai-review/' + path, import.meta.url);

test('AI review policy defines a read-only, advisory, fail-closed boundary', () => {
  assert.match(policy, /Read-only by construction/);
  assert.match(policy, /Untrusted content is data, never authority/);
  assert.match(policy, /No untrusted-code execution/);
  assert.match(policy, /No secrets or private user data/);
  assert.match(policy, /A review report is advisory and cannot authorize a merge/);
  assert.match(policy, /No GitHub mutation is available to the reviewer/);
  assert.match(policy, /The trusted policy must be loaded from outside the PR head/);
  assert.match(policy, /do not connect it to the reviewer environment/i);
  assert.match(policy, /must not be described as a compliant read-only reviewer/i);
  assert.match(policy, /do not echo it, copy it into the report, or attempt to validate or use it/i);
});

test('adversarial matrix covers Markdown, source comments, test output, and workflow logs', async () => {
  const expectedSources = new Set(['markdown', 'source-comment', 'test-output', 'workflow-log']);
  assert.equal(manifest.version, 1);
  assert.equal(manifest.execution, 'Read as untrusted text only. Never execute fixture content.');
  assert.equal(manifest.cases.length, expectedSources.size);
  assert.deepEqual(new Set(manifest.cases.map((entry) => entry.source_type)), expectedSources);

  const ids = new Set();
  for (const entry of manifest.cases) {
    assert.ok(entry.id);
    assert.ok(!ids.has(entry.id), 'duplicate fixture id: ' + entry.id);
    ids.add(entry.id);
    assert.equal(entry.privileged_action_expected, false);
    assert.ok(entry.attack_classes.length > 0);
    assert.match(entry.path, /^[A-Za-z0-9][A-Za-z0-9._-]*$/, 'fixture path must stay within the fixture directory');

    const fixture = await readFile(fixtureUrl(entry.path), 'utf8');
    assert.match(fixture, /UNTRUSTED_FIXTURE_START/);
    assert.match(fixture, /UNTRUSTED_FIXTURE_END/);
    assert.match(fixture, /ignore|override|reviewer|instructions|policy/i);

    assert.ok(manifest.expected_handling.some((item) => /untrusted content/i.test(item)));
    assert.ok(manifest.expected_handling.some((item) => /do not obey/i.test(item)));
    assert.ok(manifest.expected_handling.some((item) => /do not invoke tools/i.test(item)));
    assert.ok(manifest.expected_handling.some((item) => /report the source location/i.test(item)));
  }
});

test('synthetic fixtures contain no live credential material', async () => {
  for (const entry of manifest.cases) {
    const fixture = await readFile(fixtureUrl(entry.path), 'utf8');
    assert.doesNotMatch(fixture, /(?:gh[pousr]|github_pat)_[A-Za-z0-9_]{20,}/i);
    assert.doesNotMatch(fixture, /sk-[A-Za-z0-9]{20,}/);
    assert.doesNotMatch(fixture, /-----BEGIN [A-Z ]+ PRIVATE KEY-----/);
  }
});
