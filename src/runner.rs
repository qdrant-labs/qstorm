use std::sync::Arc;
use std::time::Instant;

use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use crate::config::{BenchmarkConfig, SearchMode};
use crate::error::Result;
use crate::metrics::{BurstMetrics, Metrics};
use crate::provider::SearchProvider;
use crate::queries::EmbeddedQuery;
use crate::types::SearchParams;

/// Orchestrates benchmark execution for vector search
pub struct BenchmarkRunner {
    provider: Box<dyn SearchProvider>,
    config: BenchmarkConfig,
    metrics: Metrics,
    queries: Vec<EmbeddedQuery>,
}

impl BenchmarkRunner {
    pub fn new(provider: Box<dyn SearchProvider>, config: BenchmarkConfig) -> Self {
        Self {
            provider,
            config,
            metrics: Metrics::new(),
            queries: Vec::new(),
        }
    }

    /// Set the embedded queries to use for benchmarking
    pub fn with_queries(mut self, queries: Vec<EmbeddedQuery>) -> Self {
        self.queries = queries;
        self
    }

    /// Get the number of loaded queries
    pub fn query_count(&self) -> usize {
        self.queries.len()
    }

    /// Connect to the provider
    pub async fn connect(&mut self) -> Result<()> {
        self.provider.connect().await
    }

    /// Disconnect from the provider
    pub async fn disconnect(&mut self) -> Result<()> {
        self.provider.disconnect().await
    }

    /// Run warmup iterations (results discarded)
    pub async fn warmup(&mut self) -> Result<()> {
        if self.queries.is_empty() {
            warn!("No queries configured for warmup");
            return Ok(());
        }

        info!(
            iterations = self.config.warmup_iterations,
            "Starting warmup"
        );

        let params = SearchParams {
            top_k: self.config.top_k,
            timeout_ms: self.config.timeout_ms,
            ..Default::default()
        };

        for i in 0..self.config.warmup_iterations {
            let query = &self.queries[i % self.queries.len()];
            let _ = self.execute_query(query, &params).await;
        }

        info!("Warmup complete");
        Ok(())
    }

    /// Execute a single burst of vector queries concurrently
    pub async fn run_burst(&mut self) -> Result<BurstMetrics> {
        if self.queries.is_empty() {
            return Err(crate::error::Error::Config("No queries configured".into()));
        }

        let semaphore = Arc::new(Semaphore::new(self.config.concurrency));
        let params = Arc::new(SearchParams {
            top_k: self.config.top_k,
            timeout_ms: self.config.timeout_ms,
            ..Default::default()
        });

        self.metrics.start_burst();

        let query_indices: Vec<usize> = (0..self.config.burst_size)
            .map(|i| i % self.queries.len())
            .collect();

        // Field-level borrows so we can use &mut self.metrics after futures complete
        let provider = &*self.provider;
        let queries = &self.queries;
        let mode = self.config.mode;

        // Phase 1: dispatch all queries concurrently
        let mut futures = FuturesUnordered::new();
        for idx in query_indices {
            let sem = semaphore.clone();
            let params = params.clone();
            let query = &queries[idx];

            futures.push(async move {
                let _permit = sem.acquire_owned().await.unwrap();
                let start = Instant::now();
                let result = match mode {
                    SearchMode::Vector => provider.vector_search(&query.vector, &params).await,
                    SearchMode::Hybrid => {
                        provider
                            .hybrid_search(&query.text, &query.vector, &params)
                            .await
                    }
                };
                let latency = start.elapsed();
                (result, latency, query.text.clone())
            });
        }

        // Phase 2: collect all results
        let mut results = Vec::with_capacity(self.config.burst_size);
        while let Some(item) = futures.next().await {
            results.push(item);
        }
        drop(futures);

        // Phase 3: record metrics (requires &mut self.metrics, now safe)
        for (result, latency, query_text) in results {
            match result {
                Ok(search_results) => {
                    self.metrics.record_success(latency, None);
                    debug!(
                        latency_ms = latency.as_millis(),
                        hits = search_results.results.len(),
                        query = %query_text,
                        "Query succeeded"
                    );
                }
                Err(e) => {
                    self.metrics.record_failure(latency);
                    warn!(error = %e, latency_ms = latency.as_millis(), "Query failed");
                }
            }
        }

        self.metrics
            .finish_burst()
            .ok_or_else(|| crate::error::Error::Config("No burst in progress".into()))
    }

    /// Dispatch a query based on the configured search mode
    async fn execute_query(
        &self,
        query: &EmbeddedQuery,
        params: &SearchParams,
    ) -> crate::error::Result<crate::types::SearchResults> {
        match self.config.mode {
            SearchMode::Vector => self.provider.vector_search(&query.vector, params).await,
            SearchMode::Hybrid => {
                self.provider
                    .hybrid_search(&query.text, &query.vector, params)
                    .await
            }
        }
    }

    /// Get reference to collected metrics
    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// Get provider name
    pub fn provider_name(&self) -> &str {
        self.provider.name()
    }

    /// Get the configured search mode
    pub fn search_mode(&self) -> SearchMode {
        self.config.mode
    }

    /// Execute a custom query with payloads included (for result inspection)
    pub async fn run_custom_query(
        &self,
        query: &EmbeddedQuery,
    ) -> Result<(String, crate::types::SearchResults)> {
        let params = SearchParams {
            top_k: self.config.top_k,
            timeout_ms: self.config.timeout_ms,
            include_payload: true,
            ..Default::default()
        };

        let results = self.execute_query(query, &params).await?;
        Ok((query.text.clone(), results))
    }

    /// Execute a single sample query with payloads included (for result inspection)
    pub async fn run_sample_query(&self) -> Result<(String, crate::types::SearchResults)> {
        if self.queries.is_empty() {
            return Err(crate::error::Error::Config("No queries configured".into()));
        }

        let query = &self.queries[0];
        let params = SearchParams {
            top_k: self.config.top_k,
            timeout_ms: self.config.timeout_ms,
            include_payload: true,
            ..Default::default()
        };

        let results = self.execute_query(query, &params).await?;
        Ok((query.text.clone(), results))
    }
}
