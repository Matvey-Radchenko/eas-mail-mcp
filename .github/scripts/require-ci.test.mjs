import test from 'node:test';
import assert from 'node:assert/strict';
import { verifyRun, requiredJobs } from './require-ci.mjs';

const sha = 'a'.repeat(40);
const run = { id: 42, run_attempt: 2, head_sha: sha, status: 'completed', conclusion: 'success' };
const jobs = () => requiredJobs.map(name => ({ name, head_sha: sha, status: 'completed', conclusion: 'success' }));

test('accepts all required successful checks for the exact accepted SHA', () => {
  assert.equal(verifyRun(run, jobs(), sha).run_attempt, 2);
});
test('rejects a different commit even when its CI succeeded', () => {
  assert.throws(() => verifyRun({ ...run, head_sha: 'b'.repeat(40) }, jobs(), sha));
  assert.throws(() => verifyRun(run, jobs().map(job => ({ ...job, head_sha: 'b'.repeat(40) })), sha));
});
test('rejects skipped, failed, missing, duplicate, or running required checks', () => {
  for (const conclusion of ['failure', 'cancelled', 'skipped', null]) {
    assert.throws(() => verifyRun({ ...run, conclusion }, jobs(), sha));
    const changed = jobs();
    changed[0].conclusion = conclusion;
    assert.throws(() => verifyRun(run, changed, sha));
  }
  assert.throws(() => verifyRun(run, jobs().slice(1), sha));
  assert.throws(() => verifyRun(run, [...jobs(), jobs()[0]], sha));
  assert.throws(() => verifyRun({ ...run, status: 'in_progress' }, jobs(), sha));
});

