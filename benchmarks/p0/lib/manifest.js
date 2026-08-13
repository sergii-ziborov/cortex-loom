const { execFileSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '..', '..', '..');
const SERENA_URL = 'https://github.com/oraios/serena';

function parseCargoLock(body) {
  const versions = {};
  for (const block of body.split('[[package]]').slice(1)) {
    const name = block.match(/^name = "([^"]+)"/m);
    const version = block.match(/^version = "([^"]+)"/m);
    if (name && version) versions[name[1]] = version[1];
  }
  return versions;
}

function configuredWeavatrixVersion(config) {
  const args = (((config || {}).mcpServers || {}).weavatrix || {}).args || [];
  const packageArg = args.find((argument) => /^weavatrix@/.test(argument));
  return packageArg ? packageArg.slice('weavatrix@'.length) : null;
}

function modelIdentity(name, tagResponse, runtimeVersion, parameters, showResponse = {}) {
  const model = (tagResponse.models || []).find((candidate) => (
    candidate.name === name || candidate.model === name
  ));
  return {
    used: true,
    name,
    digest: model ? model.digest : null,
    runtime: 'ollama',
    runtimeVersion,
    parameters: { ...parameters },
    details: model ? model.details || showResponse.details || {} : showResponse.details || {},
  };
}

function modelNotUsed() {
  return {
    used: false,
    reason: 'suite does not invoke a model',
    name: null,
    digest: null,
    runtime: null,
    runtimeVersion: null,
    parameters: {},
    details: {},
  };
}

function commandInvocation(command, args, platform = process.platform, comspec = process.env.ComSpec) {
  if (platform === 'win32' && /\.(?:cmd|bat)$/i.test(command)) {
    if (!comspec) throw new Error('ComSpec is required to execute Windows command scripts');
    return { command: comspec, args: ['/d', '/c', command, ...args] };
  }
  return { command, args };
}

function execute(command, args, cwd) {
  const invocation = commandInvocation(command, args);
  return execFileSync(invocation.command, invocation.args, {
    cwd,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  }).trim();
}

function observed(operation) {
  try { return { value: operation(), reason: null }; }
  catch (error) { return { value: null, reason: error.message }; }
}

function repositoryState(repository) {
  const commit = observed(() => execute('git', ['-C', repository, 'rev-parse', 'HEAD']));
  const dirtyOutput = observed(() => execute('git', ['-C', repository, 'status', '--porcelain']));
  const remote = observed(() => execute('git', ['-C', repository, 'remote', 'get-url', 'origin']));
  return {
    path: path.resolve(repository),
    remote,
    commit,
    dirty: dirtyOutput.value === null ? null : dirtyOutput.value.length !== 0,
    dirtyReason: dirtyOutput.reason,
  };
}

function currentSerena() {
  return observed(() => {
    const output = execute('git', ['ls-remote', SERENA_URL, 'HEAD']);
    const commit = output.split(/\s+/)[0];
    if (!/^[0-9a-f]{40}$/i.test(commit)) throw new Error(`unexpected Serena HEAD: ${output}`);
    return { source: SERENA_URL, commit };
  });
}

function exactNpmWeavatrix(version) {
  return observed(() => {
    const resolved = JSON.parse(execute('npm.cmd', ['view', `weavatrix@${version}`, 'version', '--json']));
    if (resolved !== version) throw new Error(`registry resolved ${resolved}, expected ${version}`);
    return resolved;
  });
}

function dependencyVersions() {
  const versions = parseCargoLock(fs.readFileSync(path.join(ROOT, 'Cargo.lock'), 'utf8'));
  const config = JSON.parse(fs.readFileSync(path.join(ROOT, '.mcp.json'), 'utf8'));
  const names = [
    'blazingly-json',
    'mcport',
    'weavatrix-rust',
    'weavatrix-refactor-plan',
    'weavatrix-edit',
  ];
  const result = Object.fromEntries(names.map((name) => [name, versions[name] || null]));
  result['npm-weavatrix'] = configuredWeavatrixVersion(config);
  return result;
}

async function detectEnvironment({ suiteVersion, targetRepository, model, modelParameters, mcp }) {
  const { show, tags } = require('./ollama');
  const runtimeVersion = observed(() => execute('ollama', ['--version']));
  let modelManifest = null;
  if (model) {
    const [tagResponse, showResponse] = await Promise.all([tags(), show(model)]);
    modelManifest = modelIdentity(
      model,
      tagResponse,
      runtimeVersion.value,
      modelParameters || {},
      showResponse,
    );
  }
  const engines = dependencyVersions();
  return {
    reportSchema: 'cortex-benchmark.v2',
    suiteVersion,
    observedAt: new Date().toISOString(),
    command: process.argv,
    host: {
      operatingSystem: `${os.platform()} ${os.release()}`,
      architecture: os.arch(),
      node: process.version,
    },
    cortex: repositoryState(ROOT),
    target: repositoryState(targetRepository),
    engines,
    registryWeavatrix: exactNpmWeavatrix(engines['npm-weavatrix']),
    serena: currentSerena(),
    model: modelManifest || modelNotUsed(),
    mcp,
  };
}

module.exports = {
  ROOT,
  commandInvocation,
  configuredWeavatrixVersion,
  currentSerena,
  detectEnvironment,
  modelIdentity,
  modelNotUsed,
  parseCargoLock,
  repositoryState,
};
