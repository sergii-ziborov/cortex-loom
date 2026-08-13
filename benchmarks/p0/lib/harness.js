const { execFileSync } = require('node:child_process');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');

const { ROOT } = require('./manifest');

function ensureDirectory(directory) {
  fs.mkdirSync(directory, { recursive: true });
}

function sha256(body) {
  return crypto.createHash('sha256').update(body).digest('hex');
}

function writeArtifact(outputRoot, relative, body) {
  const destination = path.join(outputRoot, relative);
  ensureDirectory(path.dirname(destination));
  fs.writeFileSync(destination, body, 'utf8');
  return { path: destination, sha256: sha256(body), bytes: Buffer.byteLength(body) };
}

function execute(command, args, options = {}) {
  const started = process.hrtime.bigint();
  try {
    const stdout = execFileSync(command, args, {
      cwd: options.cwd,
      encoding: 'utf8',
      env: options.env || process.env,
      timeout: options.timeoutMs || 300_000,
      maxBuffer: options.maxBuffer || 128 * 1024 * 1024,
    });
    return {
      ok: true,
      stdout,
      stderr: '',
      latencyMs: Number(process.hrtime.bigint() - started) / 1e6,
    };
  } catch (error) {
    return {
      ok: false,
      stdout: (error.stdout || '').toString(),
      stderr: (error.stderr || error.message || '').toString(),
      latencyMs: Number(process.hrtime.bigint() - started) / 1e6,
    };
  }
}

function rg(repository, args) {
  return execute('rg', args, { cwd: repository }).stdout;
}

function rustFiles(repository, directories) {
  const found = [];
  function walk(relative) {
    const absolute = path.join(repository, relative);
    for (const entry of fs.readdirSync(absolute, { withFileTypes: true })) {
      const child = path.join(relative, entry.name);
      if (entry.isDirectory()) walk(child);
      else if (entry.name.endsWith('.rs')) found.push(child.replace(/\\/g, '/'));
    }
  }
  for (const directory of directories) walk(directory);
  found.sort();
  return found;
}

function safeWorktreePath(destination) {
  const allowedRoots = [
    path.resolve(ROOT, '.cortex-loom', 'bench', 'p0', 'worktrees'),
    path.resolve(ROOT, '..', 'weavatrix-search', '.cortex-loom', 'bench', 'p0', 'worktrees'),
  ];
  const resolved = path.resolve(destination);
  if (!allowedRoots.some((allowed) => resolved !== allowed && resolved.startsWith(`${allowed}${path.sep}`))) {
    throw new Error(`refusing benchmark worktree outside ${allowedRoots.join(' or ')}: ${resolved}`);
  }
  return resolved;
}

function createWorktree(source, destination, commit) {
  const resolved = safeWorktreePath(destination);
  ensureDirectory(path.dirname(resolved));
  if (fs.existsSync(resolved)) {
    execute('git', ['-C', source, 'worktree', 'remove', '--force', resolved], { timeoutMs: 120_000 });
  }
  const result = execute('git', ['-C', source, 'worktree', 'add', '--detach', resolved, commit], { timeoutMs: 120_000 });
  if (!result.ok) throw new Error(`worktree add failed: ${result.stderr}`);
  return resolved;
}

function resetWorktree(repository, commit, options = {}) {
  const resolved = safeWorktreePath(repository);
  const reset = execute('git', ['-C', resolved, 'reset', '--hard', commit], { timeoutMs: 120_000 });
  if (!reset.ok) throw new Error(`worktree reset failed: ${reset.stderr}`);
  if (options.clean !== false) {
    const clean = execute('git', ['-C', resolved, 'clean', '-fdx'], { timeoutMs: 120_000 });
    if (!clean.ok) throw new Error(`worktree clean failed: ${clean.stderr}`);
  }
}

function removeWorktree(source, destination) {
  const resolved = safeWorktreePath(destination);
  if (!fs.existsSync(resolved)) return;
  const result = execute('git', ['-C', source, 'worktree', 'remove', '--force', resolved], { timeoutMs: 120_000 });
  if (!result.ok) throw new Error(`worktree remove failed: ${result.stderr}`);
}

function writeReport(destination, report) {
  ensureDirectory(path.dirname(destination));
  fs.writeFileSync(destination, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  return destination;
}

function archiveCurrentReport(destination) {
  if (!fs.existsSync(destination)) return null;
  const body = fs.readFileSync(destination, 'utf8');
  const previous = JSON.parse(body);
  const historicalDirectory = path.join(path.dirname(destination), 'historical');
  const extension = path.extname(destination);
  const base = path.basename(destination, extension);
  const historicalPath = path.join(
    historicalDirectory,
    `${base}-${sha256(body).slice(0, 12)}${extension}`,
  );
  const historical = {
    ...previous,
    historical: true,
    supersededBy: path.basename(destination),
  };
  writeReport(historicalPath, historical);
  writeReport(destination, historical);
  return historicalPath;
}

function writeCurrentReport(destination, report) {
  archiveCurrentReport(destination);
  return writeReport(destination, { ...report, historical: false });
}

module.exports = {
  archiveCurrentReport,
  createWorktree,
  ensureDirectory,
  execute,
  removeWorktree,
  resetWorktree,
  rg,
  rustFiles,
  sha256,
  writeArtifact,
  writeCurrentReport,
  writeReport,
};
