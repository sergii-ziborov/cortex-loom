const http = require('node:http');

function request(pathname, method = 'GET', payload) {
  return new Promise((resolve, reject) => {
    const body = payload === undefined ? '' : JSON.stringify(payload);
    const req = http.request({
      host: '127.0.0.1',
      port: 11434,
      path: pathname,
      method,
      headers: body ? {
        'content-type': 'application/json',
        'content-length': Buffer.byteLength(body),
      } : {},
    }, (response) => {
      let data = '';
      response.on('data', (chunk) => { data += chunk; });
      response.on('end', () => resolve({ status: response.statusCode, data }));
    });
    req.on('error', reject);
    req.setTimeout(0);
    if (body) req.write(body);
    req.end();
  });
}

async function jsonRequest(pathname, method = 'GET', payload) {
  const response = await request(pathname, method, payload);
  if (response.status !== 200) {
    throw new Error(`ollama ${response.status}: ${response.data.slice(0, 300)}`);
  }
  return JSON.parse(response.data);
}

async function generate(model, prompt, options = {}) {
  const parameters = {
    temperature: 0,
    num_ctx: 32_768,
    num_predict: 400,
    seed: 7,
    ...options,
  };
  const base = { model, prompt, stream: false, keep_alive: '30m', options: parameters };
  let response = await request('/api/generate', 'POST', { ...base, think: false });
  if (response.status === 400) response = await request('/api/generate', 'POST', base);
  if (response.status !== 200) {
    throw new Error(`ollama ${response.status}: ${response.data.slice(0, 300)}`);
  }
  const value = JSON.parse(response.data);
  return {
    answer: value.response || '',
    thinking: value.thinking || '',
    promptTokens: value.prompt_eval_count || 0,
    promptMs: Math.round((value.prompt_eval_duration || 0) / 1e6),
    generationTokens: value.eval_count || 0,
    generationMs: Math.round((value.eval_duration || 0) / 1e6),
    totalMs: Math.round((value.total_duration || 0) / 1e6),
    parameters,
  };
}

async function tags() {
  return jsonRequest('/api/tags');
}

async function show(model) {
  return jsonRequest('/api/show', 'POST', { model });
}

module.exports = { generate, show, tags };
