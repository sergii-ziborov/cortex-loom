const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const ROOT = path.resolve(__dirname, '..', '..', '..');

function initialize(command, args) {
  return new Promise((resolve, reject) => {
    const executable = process.platform === 'win32' ? process.env.ComSpec : command;
    const childArgs = process.platform === 'win32'
      ? ['/d', '/c', command, ...args]
      : args;
    const child = spawn(executable, childArgs, {
      cwd: ROOT,
      shell: false,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    let buffer = '';
    let stderr = '';
    const timeout = setTimeout(() => {
      child.kill();
      reject(new Error(`configured MCP initialize timed out: ${stderr}`));
    }, 120_000);
    child.stderr.on('data', (chunk) => { stderr += chunk.toString('utf8'); });
    child.stdout.on('data', (chunk) => {
      buffer += chunk.toString('utf8');
      let newline;
      while ((newline = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, newline).trim();
        buffer = buffer.slice(newline + 1);
        if (!line) continue;
        let message;
        try { message = JSON.parse(line); } catch { continue; }
        if (message.id === 1) {
          clearTimeout(timeout);
          child.kill();
          resolve(message);
          return;
        }
      }
    });
    child.on('error', (error) => {
      clearTimeout(timeout);
      reject(error);
    });
    child.stdin.write(`${JSON.stringify({
      jsonrpc: '2.0',
      id: 1,
      method: 'initialize',
      params: {
        protocolVersion: '2025-06-18',
        capabilities: {},
        clientInfo: { name: 'cortex-p0-config-test', version: '1' },
      },
    })}\n`);
  });
}

test('configured Weavatrix server identifies itself as 1.7.0', async () => {
  const config = JSON.parse(fs.readFileSync(path.join(ROOT, '.mcp.json'), 'utf8'));
  assert.equal(config.mcpServers.superpowers, undefined);
  const server = config.mcpServers.weavatrix;
  const response = await initialize(server.command, server.args);

  assert.equal(response.error, undefined);
  assert.equal(response.result.serverInfo.version, '1.7.0');
});
