function classifyFailure(facts) {
  if (facts.taskSuccess) return null;
  if (facts.harnessValid === false) return 'HARNESS_BUG';
  if (facts.truthPresent && facts.rawQueryExpectedLossless && !facts.rawWeavatrixPresent) {
    return 'WEAVATRIX_BUG';
  }
  if (facts.rawWeavatrixPresent && !facts.cortexPresent) return 'CORTEX_BUG';
  if (facts.cortexPresent && facts.modelHadEvidence) return 'MODEL_FAILURE';
  return 'UNCLASSIFIED';
}

function median(values) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

function summarizeRows(rows) {
  const grouped = new Map();
  for (const row of rows) {
    const key = `${row.suite}\u0000${row.task}\u0000${row.arm}`;
    if (!grouped.has(key)) grouped.set(key, []);
    grouped.get(key).push({
      ...row,
      falseConfidence: typeof row.falseConfidence === 'boolean'
        ? row.falseConfidence
        : row.sufficient === true && row.taskSuccess === false,
    });
  }
  const groups = [...grouped.values()].map((samples) => {
    const latencies = samples.map((row) => row.latencyMs).filter(Number.isFinite);
    return {
      suite: samples[0].suite,
      task: samples[0].task,
      arm: samples[0].arm,
      samples,
      qualityEarned: samples.reduce((sum, row) => sum + (row.qualityEarned || 0), 0),
      qualityPossible: samples.reduce((sum, row) => sum + (row.qualityPossible || 0), 0),
      falseConfidence: samples.filter((row) => row.falseConfidence).length,
      latencyMs: latencies.length === 0 ? null : {
        median: median(latencies),
        min: Math.min(...latencies),
        max: Math.max(...latencies),
      },
    };
  });
  groups.sort((a, b) => `${a.suite}/${a.task}/${a.arm}`.localeCompare(`${b.suite}/${b.task}/${b.arm}`));
  return {
    groups,
    hasUnclassified: rows.some((row) => !row.taskSuccess && row.failureClass === 'UNCLASSIFIED'),
  };
}

module.exports = { classifyFailure, summarizeRows };
