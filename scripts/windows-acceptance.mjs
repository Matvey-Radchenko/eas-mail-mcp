// Run with Windows Node inside the dedicated acceptance bottle or on Windows CI.
// Inputs are the installed npm prefix and same-commit synthetic harness directory.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { startRpc, npmShimCommand } from './windows-acceptance/stdio.mjs';

assert.equal(process.platform, 'win32', 'Acceptance requires Windows Node');
assert.equal(process.arch, 'x64');
const [prefixArgument, harnessArgument] = process.argv.slice(2);
assert(prefixArgument && harnessArgument, 'Provide installed npm prefix and harness directory');
const prefix = resolve(prefixArgument);
const harness = resolve(harnessArgument);
const launcher = join(prefix, 'node_modules', 'eas-mail-mcp', 'bin', 'eas-mail-mcp.js');
const contract = JSON.parse(readFileSync(new URL('../contracts/v1.0.json', import.meta.url)));
const expectedNames = Object.keys(contract.mcp).sort();
const state = mkdtempSync(join(tmpdir(), 'eas-mail-acceptance-'));
const env = { ...process.env, EAS_MAIL_HARNESS_STATE_DIR: state };

function run(binary, args, options = {}) {
  const result = spawnSync(binary, args, { ...options, env, encoding: 'utf8', timeout: 30_000, maxBuffer: 2 ** 20 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, 'CLI acceptance command failed');
  return result.stdout.trim();
}

function hash(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

async function session(full) {
  const rpc = startRpc(join(harness, 'harness-server.exe'), [], { env });
  const { request } = rpc;
  try {
    await request('initialize', { protocolVersion: '2025-03-26', capabilities: {},
      clientInfo: { name: 'windows-synthetic-acceptance', version: '1.0.0' } });
    rpc.notify('notifications/initialized');
    if (full) {
      const tools = await request('tools/list', {});
      assert.deepEqual(tools.tools.map(tool => tool.name).sort(), expectedNames);
      const read = await request('tools/call', { name: 'mail_list', arguments: { limit: 1 } });
      assert.notEqual(read.isError, true);
      assert.equal(read.structuredContent.error, null);
      assert.equal(read.structuredContent.data.items.length, 1);
      const writeArgs = { mail_ref: read.structuredContent.data.items[0].mail_ref,
        is_read: true, idempotency_key: randomUUID() };
      for (let attempt = 0; attempt < 2; attempt++) {
        const changed = await request('tools/call', { name: 'mail_mark_read', arguments: writeArgs });
        assert.notEqual(changed.isError, true);
        assert.equal(changed.structuredContent.data.status, 'succeeded');
      }
      const failed = await request('tools/call', { name: 'mail_get', arguments: { mail_ref: 'invalid' } });
      assert.equal(failed.isError, true);
      assert(failed.structuredContent.error.code);
    }
    await rpc.close();
  } finally { await rpc.dispose(); }
}

try {
  const version = run(process.execPath, [launcher, '--version']);
  assert.equal(version, 'eas-mail-mcp 1.0.0');
  const shim = npmShimCommand(join(prefix, 'eas-mail-mcp.cmd'), '--version');
  assert.equal(run(shim.binary, shim.args, { windowsVerbatimArguments: shim.windowsVerbatimArguments }), version);
  const native = run(process.execPath, [launcher, 'native-path']);
  assert(native.includes('eas-mail-mcp-windows-x64') && native.endsWith('.exe'));
  run(native, ['--help']);
  const cli = join(harness, 'harness-cli.exe');
  const mail = JSON.parse(run(cli, ['mail', 'list', '--limit', '2']));
  assert.equal(mail.error, null);
  assert.equal(mail.data.items.length, 2);
  const changed = JSON.parse(run(cli, ['mail', 'set-flag', mail.data.items[0].mail_ref,
    'active', '--idempotency-key', randomUUID(), '--yes']));
  assert.equal(changed.data.status, 'succeeded');
  for (let cycle = 0; cycle < 24; cycle++) await session(cycle === 0);
  process.stdout.write(JSON.stringify({
    platform: process.platform, architecture: process.arch, version: '1.0.0',
    native_sha256: hash(native), cli_harness_sha256: hash(cli),
    mcp_harness_sha256: hash(join(harness, 'harness-server.exe')),
    tool_count: expectedNames.length, clean_stdio_sessions: 24,
    packaged_cli: true, npm_cmd_shim: true, synthetic_cli: true, synthetic_mcp: true,
    credential_manager: 'not_verified', live_exchange: 'not_verified',
  }, null, 2) + '\n');
} finally { rmSync(state, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }); }
