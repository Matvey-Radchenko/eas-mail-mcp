import { pathToFileURL } from 'node:url';

export const requiredJobs = Object.freeze([
  'quality', 'windows-quality', 'npm-pack (macOS)', 'npm-pack (Windows)',
]);

export function verifyRun(run, jobs, sha) {
  if (!run || run.head_sha !== sha || run.status !== 'completed' || run.conclusion !== 'success') {
    throw new Error('The accepted SHA requires a completed successful CI run.');
  }
  for (const name of requiredJobs) {
    const matching = jobs.filter(job => job.name === name);
    if (matching.length !== 1 || matching[0].head_sha !== sha ||
        matching[0].status !== 'completed' || matching[0].conclusion !== 'success') {
      throw new Error('The accepted SHA is missing a successful required CI job: ' + name);
    }
  }
  return { sha, run_id: run.id, run_attempt: run.run_attempt, required_jobs: [...requiredJobs] };
}

export async function main() {
  const { GITHUB_REPOSITORY: repository, GITHUB_SHA: sha, GH_TOKEN: token } = process.env;
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository ?? '') || !/^[a-f0-9]{40}$/.test(sha ?? '') || !token) {
    throw new Error('Release CI verification requires repository, exact SHA and a read-only token.');
  }
  async function api(path) {
    const response = await fetch('https://api.github.com/repos/' + repository + path, {
      headers: { Authorization: 'Bearer ' + token, Accept: 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28' },
      signal: AbortSignal.timeout(30_000),
    });
    if (!response.ok) throw new Error('GitHub CI verification failed with HTTP ' + response.status);
    return response.json();
  }
  const runs = await api('/actions/workflows/ci.yml/runs?head_sha=' + sha + '&per_page=100');
  const candidates = (runs.workflow_runs ?? []).filter(run => run.head_sha === sha);
  candidates.sort((a, b) => b.id - a.id);
  const run = candidates[0];
  if (!run || run.status !== 'completed' || run.conclusion !== 'success') {
    throw new Error('The newest CI run for this exact SHA has not succeeded.');
  }
  const jobs = [];
  for (let page = 1; page <= 10; page++) {
    const result = await api('/actions/runs/' + run.id + '/attempts/' + run.run_attempt + '/jobs?per_page=100&page=' + page);
    jobs.push(...(result.jobs ?? []));
    if (jobs.length >= result.total_count) break;
    if (page === 10) throw new Error('CI job pagination exceeded its safety limit.');
  }
  const acceptance = verifyRun(run, jobs, sha);
  process.stdout.write(JSON.stringify(acceptance, null, 2) + '\n');
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}

