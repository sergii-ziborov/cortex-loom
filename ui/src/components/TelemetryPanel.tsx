import { useCallback, useEffect, useState } from 'react'
import { fetchTelemetry } from '../api/client'
import type { ShadowAggregate, TelemetrySnapshot } from '../types'

interface TelemetryPanelProps {
  onClose: () => void
}

function thousands(value: number): string {
  return value.toLocaleString('en-US')
}

function percent(value: number): string {
  return `${Math.round(value * 100)}%`
}

function device(aggregate: ShadowAggregate): string {
  const entries = Object.entries(aggregate.devices)
  if (entries.length === 0) return '—'
  return entries.map(([name, count]) => `${name}×${count}`).join(' ')
}

/**
 * Read-only view of how the model actually interacted with the deterministic
 * pipeline: where work was routed, what the evidence packets cost, and what
 * the shadow model said next to the deterministic decision.
 *
 * Nothing here can change workflow state — the panel only reads what the host
 * already recorded.
 */
export function TelemetryPanel({ onClose }: TelemetryPanelProps) {
  const [snapshot, setSnapshot] = useState<TelemetrySnapshot | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setSnapshot(await fetchTelemetry())
      setError(null)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const usage = snapshot?.usage
  const quality = snapshot?.quality
  const routedAway = usage && usage.routeCalls > 0
    ? usage.routedAwayFromUpstream / usage.routeCalls
    : 0

  return (
    <div className="dialog-backdrop" role="dialog" aria-modal="true" aria-label="Model interaction">
      <div className="dialog telemetry-dialog">
        <header className="dialog-header">
          <h2>Model interaction</h2>
          <div className="header-spacer" />
          <button className="ghost-button" type="button" onClick={() => void load()} disabled={loading}>
            {loading ? 'Reading…' : 'Refresh'}
          </button>
          <button className="icon-button" type="button" onClick={onClose} aria-label="Close">×</button>
        </header>

        {error && <p className="dialog-error">{error}</p>}
        {!error && !snapshot && loading && <div className="loader" aria-label="Loading" />}

        {snapshot && usage && quality && (
          <div className="telemetry-body">
            <section className="telemetry-section">
              <h3>Routing</h3>
              <p className="telemetry-hint">
                Work the deterministic policy kept away from the upstream agent. This is the
                measured lever; packet compression is secondary.
              </p>
              <div className="telemetry-metrics">
                <Metric label="Routing calls" value={thousands(usage.routeCalls)} />
                <Metric
                  label="Away from upstream"
                  value={`${thousands(usage.routedAwayFromUpstream)} (${percent(routedAway)})`}
                  emphasis
                />
                <Metric label="Compilations" value={thousands(usage.compileCalls)} />
                <Metric
                  label="Compile latency p50 / p95"
                  value={`${usage.compileLatencyP50Ms} / ${usage.compileLatencyP95Ms} ms`}
                />
              </div>
            </section>

            <section className="telemetry-section">
              <h3>Evidence budget</h3>
              <p className="telemetry-hint">
                Omitted tokens are evidence assembled and then dropped to fit the budget — an
                omission volume, not a measured saving.
              </p>
              <div className="telemetry-metrics">
                <Metric label="Assembled" value={thousands(usage.rawTokensTotal)} />
                <Metric label="Sent" value={thousands(usage.selectedTokensTotal)} emphasis />
                <Metric label="Omitted" value={thousands(usage.omittedTokensTotal)} />
                <Metric label="Escalated upstream" value={thousands(usage.requiresUpstreamCount)} />
              </div>
              <div className="telemetry-metrics">
                <Metric
                  label="Upstream reported in / out"
                  value={`${thousands(usage.upstreamInputTokensTotal)} / ${thousands(usage.upstreamOutputTokensTotal)}`}
                />
                <Metric
                  label="Creditable (clean runs)"
                  value={thousands(quality.qualityEquivalentOmittedTokens)}
                />
                <Metric label="Unproven" value={thousands(quality.unprovenOmittedTokens)} />
                <Metric label="Unattributed samples" value={thousands(quality.unattributedSamples)} />
              </div>
            </section>

            <section className="telemetry-section">
              <h3>Shadow models</h3>
              <p className="telemetry-hint">
                What a local model would have decided, recorded next to the deterministic result.
                It never influences the workflow; a missed escalation is the metric that matters.
              </p>
              {snapshot.shadow.length === 0 ? (
                <p className="telemetry-empty">
                  No shadow samples. Set <code>CORTEX_SHADOW=1</code> and a model tag on the MCP host.
                </p>
              ) : (
                <table className="telemetry-table">
                  <thead>
                    <tr>
                      <th>Operation</th><th>Model</th><th>Samples</th><th>Schema</th>
                      <th>Agreement</th><th>Missed escalations</th><th>p50 / p95</th><th>Device</th>
                    </tr>
                  </thead>
                  <tbody>
                    {snapshot.shadow.map(row => (
                      <tr key={`${row.operation}:${row.modelTag}`}>
                        <td>{row.operation}</td>
                        <td><code>{row.modelTag}</code></td>
                        <td>{row.samples}</td>
                        <td>{percent(row.schemaValidRate)}</td>
                        <td>{percent(row.agreementRate)}</td>
                        <td className={row.missedEscalations > 0 ? 'telemetry-alarm' : 'telemetry-ok'}>
                          {row.missedEscalations}
                        </td>
                        <td>{row.latencyP50Ms} / {row.latencyP95Ms} ms</td>
                        <td>{device(row)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </section>

            {snapshot.shadowSamples.length > 0 && (
              <section className="telemetry-section">
                <h3>Deterministic vs. shadow</h3>
                <table className="telemetry-table">
                  <thead>
                    <tr><th>Operation</th><th>Deterministic</th><th>Shadow</th><th>Verdict</th></tr>
                  </thead>
                  <tbody>
                    {snapshot.shadowSamples.map(sample => (
                      <tr key={sample.id}>
                        <td>{sample.operation}</td>
                        <td><code>{sample.deterministicSummary}</code></td>
                        <td><code>{sample.shadowSummary ?? sample.error ?? '—'}</code></td>
                        <td className={sample.missedEscalation ? 'telemetry-alarm' : 'telemetry-ok'}>
                          {sample.missedEscalation
                            ? 'missed escalation'
                            : sample.agreement === true
                              ? 'agreed'
                              : sample.schemaValid === false ? 'invalid' : 'differed'}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </section>
            )}

            {snapshot.usageSamples.length > 0 && (
              <section className="telemetry-section">
                <h3>Recent decisions</h3>
                <table className="telemetry-table">
                  <thead>
                    <tr><th>Operation</th><th>Target / budget</th><th>Sent</th><th>Omitted</th><th>Run</th></tr>
                  </thead>
                  <tbody>
                    {snapshot.usageSamples.map(sample => (
                      <tr key={sample.id}>
                        <td>{sample.operation}</td>
                        <td>
                          {sample.target
                            ? <code>{sample.target}{sample.modelTier ? ` · ${sample.modelTier}` : ''}</code>
                            : sample.budgetTokens !== null ? `${thousands(sample.budgetTokens)} budget` : '—'}
                        </td>
                        <td>{sample.selectedTokens === null ? '—' : thousands(sample.selectedTokens)}</td>
                        <td>{sample.omittedTokens === null ? '—' : thousands(sample.omittedTokens)}</td>
                        <td>{sample.runId ?? '—'}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </section>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

function Metric({ label, value, emphasis }: { label: string; value: string; emphasis?: boolean }) {
  return (
    <div className={emphasis ? 'telemetry-metric emphasis' : 'telemetry-metric'}>
      <span className="telemetry-metric-label">{label}</span>
      <strong className="telemetry-metric-value">{value}</strong>
    </div>
  )
}
