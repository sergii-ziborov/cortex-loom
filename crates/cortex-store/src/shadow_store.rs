//! Append-only shadow observation samples.
//!
//! Samples are measurement data with zero workflow authority. The store
//! exposes inserts, bounded reads, and bounded aggregates — no update or
//! delete surface exists on purpose.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use super::{StoreError, unix_timestamp};

/// Aggregates cover at most this many most-recent samples per query.
const AGGREGATE_WINDOW: usize = 1_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShadowOperation {
    RouteClassification,
    ContextCompression,
}

impl ShadowOperation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RouteClassification => "route_classification",
            Self::ContextCompression => "context_compression",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "route_classification" => Some(Self::RouteClassification),
            "context_compression" => Some(Self::ContextCompression),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShadowSample {
    pub operation: ShadowOperation,
    pub model_tag: String,
    pub device: Option<String>,
    pub latency_ms: Option<u64>,
    pub input_digest: String,
    /// Compact JSON snapshot of the deterministic outcome.
    pub deterministic_summary: String,
    /// Compact JSON of the shadow draft; `None` on failure.
    pub shadow_summary: Option<String>,
    pub schema_valid: Option<bool>,
    pub agreement: Option<bool>,
    pub missed_escalation: bool,
    pub citation_preserved_ratio: Option<f64>,
    pub hallucinated_citations: Option<u32>,
    pub token_estimate_delta: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShadowSampleRow {
    pub id: i64,
    pub created_at: i64,
    #[serde(flatten)]
    pub sample: ShadowSample,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShadowAggregate {
    pub operation: ShadowOperation,
    pub model_tag: String,
    pub samples: u32,
    pub schema_valid: u32,
    pub schema_valid_rate: f64,
    pub agreements: u32,
    pub agreement_rate: f64,
    pub missed_escalations: u32,
    pub hallucinated_total: u32,
    pub mean_preserved_ratio: Option<f64>,
    pub latency_p50_ms: u64,
    pub latency_p95_ms: u64,
    pub devices: BTreeMap<String, u32>,
}

#[derive(Clone)]
pub struct ShadowStore {
    connection: Arc<Mutex<Connection>>,
}

impl ShadowStore {
    pub(super) const fn new(connection: Arc<Mutex<Connection>>) -> Self {
        Self { connection }
    }

    /// Append one immutable sample and return its row id.
    pub fn insert(&self, sample: &ShadowSample) -> Result<i64, StoreError> {
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO shadow_samples
             (created_at, operation, model_tag, device, latency_ms, input_digest,
              deterministic_summary, shadow_summary, schema_valid, agreement,
              missed_escalation, citation_preserved_ratio, hallucinated_citations,
              token_estimate_delta, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                unix_timestamp(),
                sample.operation.as_str(),
                sample.model_tag,
                sample.device,
                sample
                    .latency_ms
                    .map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                sample.input_digest,
                sample.deterministic_summary,
                sample.shadow_summary,
                sample.schema_valid,
                sample.agreement,
                sample.missed_escalation,
                sample.citation_preserved_ratio,
                sample.hallucinated_citations,
                sample.token_estimate_delta,
                sample.error,
            ],
        )?;
        Ok(connection.last_insert_rowid())
    }

    /// Read the most recent samples, newest first, bounded to 100 rows.
    pub fn list(
        &self,
        operation: Option<ShadowOperation>,
        model: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ShadowSampleRow>, StoreError> {
        let limit = i64::try_from(limit.clamp(1, 100)).unwrap_or(100);
        self.query(operation, model, limit)
    }

    /// Aggregate the most recent samples per (operation, `model_tag`) group.
    pub fn aggregate(
        &self,
        operation: Option<ShadowOperation>,
        model: Option<&str>,
    ) -> Result<Vec<ShadowAggregate>, StoreError> {
        let window = i64::try_from(AGGREGATE_WINDOW).unwrap_or(1_000);
        let rows = self.query(operation, model, window)?;
        let mut groups: BTreeMap<(String, String), Vec<&ShadowSampleRow>> = BTreeMap::new();
        for row in &rows {
            groups
                .entry((
                    row.sample.operation.as_str().to_owned(),
                    row.sample.model_tag.clone(),
                ))
                .or_default()
                .push(row);
        }
        Ok(groups
            .into_values()
            .map(|group| aggregate(&group))
            .collect())
    }

    fn query(
        &self,
        operation: Option<ShadowOperation>,
        model: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ShadowSampleRow>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT id, created_at, operation, model_tag, device, latency_ms, input_digest,
                    deterministic_summary, shadow_summary, schema_valid, agreement,
                    missed_escalation, citation_preserved_ratio, hallucinated_citations,
                    token_estimate_delta, error
             FROM shadow_samples
             WHERE (?1 IS NULL OR operation = ?1)
               AND (?2 IS NULL OR model_tag = ?2)
             ORDER BY id DESC LIMIT ?3",
        )?;
        let rows = statement
            .query_map(
                params![operation.map(ShadowOperation::as_str), model, limit],
                |row| {
                    let operation: String = row.get(2)?;
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        operation,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, Option<bool>>(9)?,
                        row.get::<_, Option<bool>>(10)?,
                        row.get::<_, bool>(11)?,
                        row.get::<_, Option<f64>>(12)?,
                        row.get::<_, Option<i64>>(13)?,
                        row.get::<_, Option<i64>>(14)?,
                        row.get::<_, Option<String>>(15)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|row| {
                let operation = ShadowOperation::parse(&row.2).ok_or_else(|| {
                    StoreError::Database(rusqlite::Error::IntegralValueOutOfRange(0, 0))
                })?;
                Ok(ShadowSampleRow {
                    id: row.0,
                    created_at: row.1,
                    sample: ShadowSample {
                        operation,
                        model_tag: row.3,
                        device: row.4,
                        latency_ms: row.5.and_then(|value| u64::try_from(value).ok()),
                        input_digest: row.6,
                        deterministic_summary: row.7,
                        shadow_summary: row.8,
                        schema_valid: row.9,
                        agreement: row.10,
                        missed_escalation: row.11,
                        citation_preserved_ratio: row.12,
                        hallucinated_citations: row.13.and_then(|value| u32::try_from(value).ok()),
                        token_estimate_delta: row.14,
                        error: row.15,
                    },
                })
            })
            .collect()
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }
}

fn aggregate(rows: &[&ShadowSampleRow]) -> ShadowAggregate {
    let first = rows[0];
    let schema_valid = rows
        .iter()
        .filter(|row| row.sample.schema_valid == Some(true))
        .count();
    let agreements = rows
        .iter()
        .filter(|row| row.sample.agreement == Some(true))
        .count();
    let ratios: Vec<f64> = rows
        .iter()
        .filter_map(|row| row.sample.citation_preserved_ratio)
        .collect();
    let mut latencies: Vec<u64> = rows
        .iter()
        .filter_map(|row| row.sample.latency_ms)
        .collect();
    latencies.sort_unstable();
    let mut devices = BTreeMap::new();
    for row in rows {
        if let Some(device) = &row.sample.device {
            *devices.entry(device.clone()).or_insert(0_u32) += 1;
        }
    }
    ShadowAggregate {
        operation: first.sample.operation,
        model_tag: first.sample.model_tag.clone(),
        samples: count_u32(rows.len()),
        schema_valid: count_u32(schema_valid),
        schema_valid_rate: ratio(schema_valid, rows.len()),
        agreements: count_u32(agreements),
        agreement_rate: ratio(agreements, rows.len()),
        missed_escalations: count_u32(
            rows.iter()
                .filter(|row| row.sample.missed_escalation)
                .count(),
        ),
        hallucinated_total: rows
            .iter()
            .filter_map(|row| row.sample.hallucinated_citations)
            .fold(0_u32, u32::saturating_add),
        mean_preserved_ratio: if ratios.is_empty() {
            None
        } else {
            Some(ratios.iter().sum::<f64>() / count_f64(ratios.len()))
        },
        latency_p50_ms: percentile(&latencies, 50),
        latency_p95_ms: percentile(&latencies, 95),
        devices,
    }
}

fn count_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn count_f64(value: usize) -> f64 {
    f64::from(count_u32(value))
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        count_f64(numerator) / count_f64(denominator)
    }
}

/// Nearest-rank percentile over pre-sorted samples; zero when empty.
fn percentile(sorted: &[u64], percentile: u8) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (sorted.len() * usize::from(percentile))
        .div_ceil(100)
        .max(1);
    sorted[rank - 1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GraphStore;

    fn sample(operation: ShadowOperation, agreement: Option<bool>) -> ShadowSample {
        ShadowSample {
            operation,
            model_tag: "qwen-test:4b".to_owned(),
            device: Some("cpu".to_owned()),
            latency_ms: Some(120),
            input_digest: "digest".to_owned(),
            deterministic_summary: "{\"tier\":\"upstream_strong\"}".to_owned(),
            shadow_summary: Some("{\"tier\":\"local_small\"}".to_owned()),
            schema_valid: Some(true),
            agreement,
            missed_escalation: agreement == Some(false),
            citation_preserved_ratio: None,
            hallucinated_citations: None,
            token_estimate_delta: None,
            error: None,
        }
    }

    #[test]
    fn samples_are_append_only_with_monotonic_ids() {
        let store = GraphStore::open_in_memory().unwrap().shadow();
        let first = store
            .insert(&sample(ShadowOperation::RouteClassification, Some(true)))
            .unwrap();
        let second = store
            .insert(&sample(ShadowOperation::RouteClassification, Some(false)))
            .unwrap();
        assert!(second > first);

        let rows = store.list(None, None, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, second, "newest first");
        assert_eq!(rows[0].sample.agreement, Some(false));

        let rendered = serde_json::to_string(&rows[0]).expect("rows serialize for MCP/HTTP");
        assert!(
            rendered.contains("\"modelTag\""),
            "flatten works: {rendered}"
        );
    }

    #[test]
    fn listing_is_bounded_and_filterable() {
        let store = GraphStore::open_in_memory().unwrap().shadow();
        for _ in 0..5 {
            store
                .insert(&sample(ShadowOperation::RouteClassification, Some(true)))
                .unwrap();
        }
        store
            .insert(&sample(ShadowOperation::ContextCompression, None))
            .unwrap();

        assert_eq!(store.list(None, None, 3).unwrap().len(), 3);
        assert_eq!(store.list(None, None, 10_000).unwrap().len(), 6);
        let compressions = store
            .list(Some(ShadowOperation::ContextCompression), None, 10)
            .unwrap();
        assert_eq!(compressions.len(), 1);
        assert!(
            store
                .list(None, Some("other-model"), 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn aggregates_report_rates_and_missed_escalations() {
        let store = GraphStore::open_in_memory().unwrap().shadow();
        store
            .insert(&sample(ShadowOperation::RouteClassification, Some(true)))
            .unwrap();
        store
            .insert(&sample(ShadowOperation::RouteClassification, Some(false)))
            .unwrap();
        let aggregates = store.aggregate(None, None).unwrap();
        assert_eq!(aggregates.len(), 1);
        let aggregate = &aggregates[0];
        assert_eq!(aggregate.samples, 2);
        assert!((aggregate.agreement_rate - 0.5).abs() < 1e-9);
        assert_eq!(aggregate.missed_escalations, 1);
        assert_eq!(aggregate.latency_p50_ms, 120);
        assert_eq!(aggregate.devices.get("cpu"), Some(&2));
    }
}
