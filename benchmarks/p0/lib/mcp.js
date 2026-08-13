const { spawn, spawnSync } = require('node:child_process');
const { isDeepStrictEqual } = require('node:util');

function estimateTokens(value) {
  return Math.ceil(Buffer.byteLength(value || '', 'utf8') / 4);
}

function parsedJson(value) {
  try { return JSON.parse(value); } catch { return undefined; }
}

function completeness(value) {
  const state = { truncated: false, fit: null, droppedItems: 0, cursor: null };
  function visit(node) {
    if (!node || typeof node !== 'object') return;
    if (node.truncated === true) state.truncated = true;
    if (node.fit === false) state.fit = false;
    else if (node.fit === true && state.fit === null) state.fit = true;
    if (Number.isFinite(node.dropped_items)) state.droppedItems += node.dropped_items;
    if (Number.isFinite(node.droppedItems)) state.droppedItems += node.droppedItems;
    if (node.nextCursor || node.next_cursor) state.cursor = node.nextCursor || node.next_cursor;
    for (const child of Object.values(node)) visit(child);
  }
  visit(value);
  return {
    ...state,
    complete: !state.truncated && state.fit !== false && state.droppedItems === 0 && !state.cursor,
  };
}

function extractPayload(result = {}) {
  const contentText = Array.isArray(result.content)
    ? result.content.filter((block) => block && block.type === 'text').map((block) => block.text || '').join('\n')
    : '';
  const structuredText = result.structuredContent === undefined
    ? ''
    : JSON.stringify(result.structuredContent);
  let format = 'empty';
  if (contentText && structuredText) {
    format = isDeepStrictEqual(parsedJson(contentText), result.structuredContent)
      ? 'mirrored'
      : 'dual-distinct';
  } else if (contentText) {
    format = 'text';
  } else if (structuredText) {
    format = 'structured';
  }
  const countedRepresentation = contentText ? 'content' : 'structuredContent';
  const countedText = contentText || structuredText;
  const structured = result.structuredContent === undefined
    ? parsedJson(contentText)
    : result.structuredContent;
  return {
    format,
    contentText,
    structuredText,
    countedRepresentation,
    countedText,
    countedTokens: estimateTokens(countedText),
    wireTokens: estimateTokens(JSON.stringify(result)),
    completeness: completeness(structured),
  };
}

function windowsCommand(command, args) {
  if (process.platform !== 'win32' || !/\.(cmd|bat)$/i.test(command)) {
    return { command, args };
  }
  return {
    command: process.env.ComSpec,
    args: ['/d', '/c', command, ...args],
  };
}

class McpClient {
  constructor({ command, args = [], cwd, name, profile, timeoutMs = 300_000 }) {
    this.definition = { command, args: [...args], cwd, name, profile };
    this.timeoutMs = timeoutMs;
    this.nextId = 1;
    this.pending = new Map();
    this.buffer = '';
    this.stderr = '';
    const executable = windowsCommand(command, args);
    this.child = spawn(executable.command, executable.args, {
      cwd,
      shell: false,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    this.child.stdout.on('data', (chunk) => this.onStdout(chunk));
    this.child.stderr.on('data', (chunk) => { this.stderr += chunk.toString('utf8'); });
    this.child.on('error', (error) => this.rejectAll(error));
    this.child.on('exit', (code) => this.rejectAll(new Error(`${name} exited ${code}: ${this.stderr}`)));
  }

  onStdout(chunk) {
    this.buffer += chunk.toString('utf8');
    let newline;
    while ((newline = this.buffer.indexOf('\n')) >= 0) {
      const line = this.buffer.slice(0, newline).trim();
      this.buffer = this.buffer.slice(newline + 1);
      if (!line) continue;
      let message;
      try { message = JSON.parse(line); } catch { continue; }
      const pending = this.pending.get(message.id);
      if (!pending) continue;
      clearTimeout(pending.timeout);
      this.pending.delete(message.id);
      pending.resolve(message);
    }
  }

  rejectAll(error) {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timeout);
      pending.reject(error);
    }
    this.pending.clear();
  }

  request(method, params = {}) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`${this.definition.name} timed out on ${method}: ${this.stderr}`));
      }, this.timeoutMs);
      this.pending.set(id, { resolve, reject, timeout });
      this.child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
    });
  }

  async start() {
    const initialized = await this.request('initialize', {
      protocolVersion: '2025-06-18',
      capabilities: {},
      clientInfo: { name: 'cortex-p0-bench', version: '2' },
    });
    if (initialized.error) throw new Error(JSON.stringify(initialized.error));
    this.initializeResult = initialized.result;
    this.child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized', params: {} })}\n`);
    let cursor;
    let pages = 0;
    const tools = [];
    const rawPages = [];
    do {
      const response = await this.request('tools/list', cursor ? { cursor } : {});
      if (response.error) throw new Error(JSON.stringify(response.error));
      const page = response.result || {};
      rawPages.push(page);
      tools.push(...(page.tools || []));
      cursor = page.nextCursor;
      pages += 1;
    } while (cursor && pages < 100);
    this.tools = tools;
    this.schemaTokens = estimateTokens(JSON.stringify(rawPages));
    return this;
  }

  async call(name, args) {
    const start = process.hrtime.bigint();
    const response = await this.request('tools/call', { name, arguments: args });
    const latencyMs = Number(process.hrtime.bigint() - start) / 1e6;
    if (response.error) throw new Error(JSON.stringify(response.error));
    return {
      name,
      args,
      latencyMs,
      isError: Boolean(response.result && response.result.isError),
      result: response.result,
      ...extractPayload(response.result),
    };
  }

  close() {
    if (!this.child || this.child.killed) return;
    if (process.platform === 'win32' && this.child.pid) {
      spawnSync('taskkill', ['/pid', String(this.child.pid), '/t', '/f'], { stdio: 'ignore' });
    } else {
      this.child.kill();
    }
  }
}

module.exports = { McpClient, estimateTokens, extractPayload };
