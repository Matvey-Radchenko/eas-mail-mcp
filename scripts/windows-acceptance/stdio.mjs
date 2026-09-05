import { spawn } from 'node:child_process';

// Shared only by acceptance scripts/tests; production executables have no test switches.
export function startRpc(binary, args = [], options = {}) {
  const { env = process.env, requestTimeoutMs = 30_000, closeTimeoutMs = 10_000,
    maxOutputBytes = 2 ** 20 } = options;
  const child = spawn(binary, args, { env, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  const pending = new Map();
  let nextId = 0;
  let buffer = Buffer.alloc(0);
  let stderrBytes = 0;
  let failure;
  let closed = false;
  let resolveClose;
  const completion = new Promise(resolve => { resolveClose = resolve; });

  function rejectPending(error) {
    for (const item of pending.values()) { clearTimeout(item.timer); item.reject(error); }
    pending.clear();
  }
  function fail(error) {
    failure ??= error;
    rejectPending(failure);
    if (!closed) child.kill();
  }
  child.once('error', error => { failure ??= error; rejectPending(failure); });
  child.stdin.on('error', fail);
  child.stderr.on('data', data => {
    stderrBytes += data.length;
    if (stderrBytes > maxOutputBytes) fail(new Error('MCP stderr exceeded its bound'));
  });
  child.stdout.on('data', data => {
    buffer = Buffer.concat([buffer, data]);
    let newline;
    while ((newline = buffer.indexOf(10)) >= 0) {
      const line = buffer.subarray(0, newline);
      buffer = buffer.subarray(newline + 1);
      if (line.length > maxOutputBytes) { fail(new Error('MCP stdout line exceeded its bound')); return; }
      try {
        const value = JSON.parse(line.toString('utf8'));
        if (value?.jsonrpc !== '2.0') throw new Error('Invalid JSON-RPC response');
        const item = pending.get(value.id);
        if (item) {
          pending.delete(value.id); clearTimeout(item.timer);
          if (value.error !== undefined) item.reject(new Error('JSON-RPC request failed'));
          else if (!Object.hasOwn(value, 'result')) item.reject(new Error('JSON-RPC result missing'));
          else item.resolve(value.result);
        }
      } catch (error) { fail(error); return; }
    }
    if (buffer.length > maxOutputBytes) fail(new Error('MCP stdout line exceeded its bound'));
  });
  child.once('close', (code, signal) => {
    closed = true;
    if (buffer.length) failure ??= new Error('MCP stdout ended with an incomplete line');
    rejectPending(failure ?? new Error('Server exited before response'));
    resolveClose({ code, signal });
  });

  function write(message) {
    if (failure) throw failure;
    if (closed || child.stdin.destroyed) throw new Error('MCP session is closed');
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', ...message }) + '\n', error => {
      if (error) fail(error);
    });
  }
  function request(method, params) {
    const id = ++nextId;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => fail(new Error('MCP request timed out')), requestTimeoutMs);
      pending.set(id, { resolve, reject, timer });
      try { write({ id, method, params }); } catch (error) { fail(error); }
    });
  }
  async function waitForClose() {
    let timer;
    try {
      return await Promise.race([completion, new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error('Server remained after stdin closed')), closeTimeoutMs);
      })]);
    } finally { clearTimeout(timer); }
  }
  async function close() {
    child.stdin.end();
    const result = await waitForClose();
    if (failure) throw failure;
    if (result.code !== 0 || result.signal !== null) throw new Error('Server did not exit cleanly');
  }
  async function dispose() {
    if (!closed) {
      child.kill();
      child.stdin.destroy();
      child.stdout.destroy();
      child.stderr.destroy();
    }
    await waitForClose();
  }
  return { request, notify: (method, params) => write({ method, params }), close, dispose };
}

// A .cmd file needs cmd.exe; reject expansion/control syntax instead of interpolating arbitrary commands.
export function npmShimCommand(shim, action) {
  if (/["%!?^&|<>\r\n]/u.test(shim) || !['--version', 'native-path'].includes(action)) {
    throw new Error('Unsafe npm shim command path or action');
  }
  return { binary: process.env.ComSpec ?? 'cmd.exe',
    args: ['/d', '/q', '/s', '/c', `""${shim}" ${action}"`], windowsVerbatimArguments: true };
}
