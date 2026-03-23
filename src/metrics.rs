use std::time::{Duration, Instant};

use hdrhistogram::Histogram;
use serde::{Deserialize, Serialize};

/// Metrics collected from a single burst of queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurstMetrics {
    /// When this burst started
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Total duration of the burst
    pub duration_ms: u64,
    /// Number of queries executed
    pub query_count: usize,
    /// Number of successful queries
    pub success_count: usize,
    /// Number of failed queries
    pub failure_count: usize,
    /// Latency percentiles in microseconds
    pub latency: LatencyMetrics,
    /// Queries per second achieved
    pub qps: f64,
    /// Recall@k if ground truth was provided
    pub recall_at_k: Option<f64>,
}

/// Latency percentiles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyMetrics {
    pub min_us: u64,
    pub max_us: u64,
    pub mean_us: f64,
    pub p50_us: u64,
    pub p90_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
}

/// Tracks metrics across multiple bursts
pub struct Metrics {
    /// Histogram for latency tracking (in microseconds)
    latency_histogram: Histogram<u64>,
    /// Individual burst results
    bursts: Vec<BurstMetrics>,
    /// Current burst state
    current_burst: Option<BurstState>,
}

struct BurstState {
    start_time: Instant,
    start_timestamp: chrono::DateTime<chrono::Utc>,
    latencies_us: Vec<u64>,
    successes: usize,
    failures: usize,
    recalls: Vec<f64>,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            // Track latencies from 1us to 60 seconds with 3 significant figures
            latency_histogram: Histogram::new_with_bounds(1, 60_000_000, 3).unwrap(),
            bursts: Vec::new(),
            current_burst: None,
        }
    }

    /// Start tracking a new burst
    pub fn start_burst(&mut self) {
        self.current_burst = Some(BurstState {
            start_time: Instant::now(),
            start_timestamp: chrono::Utc::now(),
            latencies_us: Vec::new(),
            successes: 0,
            failures: 0,
            recalls: Vec::new(),
        });
    }

    /// Record a successful query execution
    pub fn record_success(&mut self, latency: Duration, recall: Option<f64>) {
        if let Some(burst) = &mut self.current_burst {
            let latency_us = latency.as_micros() as u64;
            burst.latencies_us.push(latency_us);
            burst.successes += 1;
            if let Some(r) = recall {
                burst.recalls.push(r);
            }
            let _ = self.latency_histogram.record(latency_us);
        }
    }

    /// Record a failed query execution
    pub fn record_failure(&mut self, latency: Duration) {
        if let Some(burst) = &mut self.current_burst {
            let latency_us = latency.as_micros() as u64;
            burst.latencies_us.push(latency_us);
            burst.failures += 1;
            let _ = self.latency_histogram.record(latency_us);
        }
    }

    /// Finish the current burst and compute metrics
    pub fn finish_burst(&mut self) -> Option<BurstMetrics> {
        let burst = self.current_burst.take()?;
        let duration = burst.start_time.elapsed();
        let duration_ms = duration.as_millis() as u64;

        let query_count = burst.successes + burst.failures;
        let qps = if duration_ms > 0 {
            (query_count as f64) / (duration_ms as f64 / 1000.0)
        } else {
            0.0
        };

        let latency = compute_latency_metrics(&burst.latencies_us);

        let recall_at_k = if burst.recalls.is_empty() {
            None
        } else {
            Some(burst.recalls.iter().sum::<f64>() / burst.recalls.len() as f64)
        };

        let metrics = BurstMetrics {
            timestamp: burst.start_timestamp,
            duration_ms,
            query_count,
            success_count: burst.successes,
            failure_count: burst.failures,
            latency,
            qps,
            recall_at_k,
        };

        self.bursts.push(metrics.clone());
        Some(metrics)
    }

    /// Get all burst metrics
    pub fn bursts(&self) -> &[BurstMetrics] {
        &self.bursts
    }

    /// Get the most recent burst
    pub fn last_burst(&self) -> Option<&BurstMetrics> {
        self.bursts.last()
    }

    /// Compute aggregate latency metrics across all bursts
    pub fn aggregate_latency(&self) -> LatencyMetrics {
        LatencyMetrics {
            min_us: self.latency_histogram.min(),
            max_us: self.latency_histogram.max(),
            mean_us: self.latency_histogram.mean(),
            p50_us: self.latency_histogram.value_at_quantile(0.50),
            p90_us: self.latency_histogram.value_at_quantile(0.90),
            p95_us: self.latency_histogram.value_at_quantile(0.95),
            p99_us: self.latency_histogram.value_at_quantile(0.99),
        }
    }

    /// Total queries executed across all bursts
    pub fn total_queries(&self) -> usize {
        self.bursts.iter().map(|b| b.query_count).sum()
    }

    /// Average QPS across all bursts
    pub fn average_qps(&self) -> f64 {
        if self.bursts.is_empty() {
            return 0.0;
        }
        self.bursts.iter().map(|b| b.qps).sum::<f64>() / self.bursts.len() as f64
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

fn compute_latency_metrics(latencies_us: &[u64]) -> LatencyMetrics {
    if latencies_us.is_empty() {
        return LatencyMetrics {
            min_us: 0,
            max_us: 0,
            mean_us: 0.0,
            p50_us: 0,
            p90_us: 0,
            p95_us: 0,
            p99_us: 0,
        };
    }

    let mut sorted = latencies_us.to_vec();
    sorted.sort_unstable();

    let min_us = sorted[0];
    let max_us = sorted[sorted.len() - 1];
    let mean_us = sorted.iter().sum::<u64>() as f64 / sorted.len() as f64;

    let percentile = |p: f64| -> u64 {
        let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
        sorted[idx]
    };

    LatencyMetrics {
        min_us,
        max_us,
        mean_us,
        p50_us: percentile(50.0),
        p90_us: percentile(90.0),
        p95_us: percentile(95.0),
        p99_us: percentile(99.0),
    }
}

/// Calculate recall@k given returned IDs and expected IDs
pub fn recall_at_k(returned: &[&str], expected: &[String], k: usize) -> f64 {
    if expected.is_empty() {
        return 0.0;
    }

    let k = k.min(expected.len());
    let expected_set: std::collections::HashSet<&str> =
        expected.iter().take(k).map(|s| s.as_str()).collect();

    let hits = returned
        .iter()
        .take(k)
        .filter(|id| expected_set.contains(*id))
        .count();

    hits as f64 / k as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recall_at_k() {
        let returned = vec!["a", "b", "c", "d", "e"];
        let expected = vec!["a".to_string(), "c".to_string(), "e".to_string()];

        // All 3 expected are in top 5
        assert!((recall_at_k(&returned, &expected, 5) - 1.0).abs() < 0.001);

        // 2 of 3 expected in top 3 (a, c)
        assert!((recall_at_k(&returned, &expected, 3) - (2.0 / 3.0)).abs() < 0.001);
    }
}
