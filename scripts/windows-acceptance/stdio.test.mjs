import assert from 'node:assert/strict';
import { test } from 'node:test';
import { startRpc, npmShimCommand } from './stdio.mjs';

const echo = `const rl=require('node:readline').createInterface({input:process.stdin});
rl.on('line', line => {const v=JSON.parse(line);if(v.id) process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:v.id,result:{method:v.method}})+'\\n');});`;

async function withRpc(script, action, options = {}) {
  const rpc = startRpc(process.execPath, ['-e', script], { requestTimeoutMs: 5000, closeTimeoutMs: 5000, ...options });
  try { await action(rpc); } finally { await rpc.dispose(); }
}

test('stdio resolves concurrent requests and exits after EOF', async () => {
  await withRpc(echo, async rpc => {
    rpc.notify('notifications/initialized');
    assert.deepEqual(await Promise.all([rpc.request('first', {}), rpc.request('second', {})]), [{method:'first'}, {method:'second'}]);
    await rpc.close();
  });
});

test('missing executable rejects immediately and is reaped without an unhandled rejection', async () => {
  const rpc = startRpc('eas-mail-acceptance-deliberately-missing-executable', [], { requestTimeoutMs: 5000, closeTimeoutMs: 5000 });
  try { await assert.rejects(rpc.request('initialize', {}), /ENOENT/u); }
  finally { await rpc.dispose(); }
});

test('early process exit rejects pending requests', async () => {
  await withRpc('process.exit(3)', async rpc => {
    await assert.rejects(rpc.request('initialize', {}), /exited before response/u);
  });
});

test('missing response times out and terminates the process', async () => {
  await withRpc('process.stdin.resume()', async rpc => {
    await assert.rejects(rpc.request('initialize', {}), /timed out/u);
  }, {requestTimeoutMs: 50});
});

test('malformed and oversized unterminated stdout reject pending requests', async () => {
  for (const script of ["process.stdout.write('not-json\\n');process.stdin.resume()", "process.stdout.write('x'.repeat(2000));process.stdin.resume()"] ) {
    await withRpc(script, async rpc => { await assert.rejects(rpc.request('initialize', {})); }, {maxOutputBytes: 1000});
  }
});

test('an incomplete final stdout line cannot pass clean close', async () => {
  await withRpc("process.stdin.resume();process.stdin.on('end',()=>process.stdout.write('{'))", async rpc => {
    await assert.rejects(rpc.close(), /incomplete line/u);
  });
});

test('cmd invocation preserves paths with spaces and rejects expansion syntax', () => {
  const command = npmShimCommand('C:\\Acceptance Folder\\eas-mail-mcp.cmd', '--version');
  assert.equal(command.args.at(-1), '""C:\\Acceptance Folder\\eas-mail-mcp.cmd" --version"');
  assert.equal(command.windowsVerbatimArguments, true);
  for (const path of ['C:\\%TEMP%\\x.cmd', 'C:\\x&calc.cmd', 'C:\\x"y.cmd']) assert.throws(()=>npmShimCommand(path, '--version'));
});
