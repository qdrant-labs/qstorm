use anyhow::{Result, anyhow};
use qstorm::{
    BurstMetrics, Config, EmbeddedQuery, Embedder, QueryFile, SearchResults,
    config::{ProviderConfig, ProviderKind},
    runner::BenchmarkRunner,
};

/// Which TUI view is active
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum View {
    #[default]
    Dashboard,
    Results,
}

/// A captured sample query result for display
pub struct SampleResult {
    pub query: String,
    pub results: SearchResults,
}

/// Application state
pub struct App {
    pub config: Config,
    runner: Option<BenchmarkRunner>,
    embedder: Option<Embedder>,
    queries: Vec<EmbeddedQuery>,
    pub state: AppState,
    pub view: View,
    pub history: MetricsHistory,
    pub status_message: Option<String>,
    pub last_sample: Option<SampleResult>,
    pub results_scroll: usize,
    pub query_input: String,
    pub editing: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    #[default]
    Idle,
    Connecting,
    Warming,
    Running,
    Paused,
    Error,
}

/// Rolling history of metrics for charting
pub struct MetricsHistory {
    pub bursts: Vec<BurstMetrics>,
    pub max_history: usize,
}

impl Default for MetricsHistory {
    fn default() -> Self {
        Self {
            bursts: Vec::new(),
            max_history: 100,
        }
    }
}

impl MetricsHistory {
    pub fn push(&mut self, metrics: BurstMetrics) {
        self.bursts.push(metrics);
        if self.bursts.len() > self.max_history {
            self.bursts.remove(0);
        }
    }

    pub fn latest(&self) -> Option<&BurstMetrics> {
        self.bursts.last()
    }

    pub fn qps_series(&self) -> Vec<(f64, f64)> {
        self.bursts
            .iter()
            .enumerate()
            .map(|(i, m)| (i as f64, m.qps))
            .collect()
    }

    pub fn p50_series(&self) -> Vec<(f64, f64)> {
        self.bursts
            .iter()
            .enumerate()
            .map(|(i, m)| (i as f64, m.latency.p50_us as f64 / 1000.0))
            .collect()
    }

    pub fn p99_series(&self) -> Vec<(f64, f64)> {
        self.bursts
            .iter()
            .enumerate()
            .map(|(i, m)| (i as f64, m.latency.p99_us as f64 / 1000.0))
            .collect()
    }

    pub fn recall_series(&self) -> Vec<(f64, f64)> {
        self.bursts
            .iter()
            .enumerate()
            .filter_map(|(i, m)| m.recall_at_k.map(|r| (i as f64, r * 100.0)))
            .collect()
    }
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            config,
            runner: None,
            embedder: None,
            queries: Vec::new(),
            state: AppState::Idle,
            view: View::default(),
            history: MetricsHistory::default(),
            status_message: None,
            last_sample: None,
            results_scroll: 0,
            query_input: String::new(),
            editing: false,
        })
    }

    pub fn provider_name(&self) -> &str {
        &self.config.provider.name
    }

    pub fn query_count(&self) -> usize {
        self.queries.len()
    }

    pub fn take_runner(&mut self) -> Option<BenchmarkRunner> {
        self.runner.take()
    }

    pub fn put_runner(&mut self, runner: BenchmarkRunner) {
        self.runner = Some(runner);
    }

    pub fn has_runner(&self) -> bool {
        self.runner.is_some()
    }

    /// Load queries from file and embed them
    pub async fn load_and_embed_queries(&mut self, query_file_path: &str) -> Result<()> {
        self.status_message = Some("Loading queries...".into());

        let query_file = QueryFile::from_file(query_file_path)?;
        if query_file.queries.is_empty() {
            return Err(anyhow!("Query file contains no queries"));
        }

        self.status_message = Some(format!("Embedding {} queries...", query_file.queries.len()));

        let embedding_config = self.config.embedding.clone().unwrap_or_default();
        let embedder = Embedder::from_config(&embedding_config)
            .map_err(|e| anyhow!("{e}"))?;
        self.queries = embedder
            .embed_queries(&query_file.queries)
            .await
            .map_err(|e| anyhow!("{e}"))?;
        self.embedder = Some(embedder);

        self.status_message = Some(format!("Loaded {} queries", self.queries.len()));
        Ok(())
    }

    pub async fn connect(&mut self) -> Result<()> {
        self.state = AppState::Connecting;
        self.status_message = Some("Connecting to provider...".into());

        let provider = create_provider(&self.config.provider)?;
        let runner = BenchmarkRunner::new(provider, self.config.benchmark.clone())
            .with_queries(self.queries.clone());

        let mut runner = runner;
        runner.connect().await?;
        self.runner = Some(runner);
        self.state = AppState::Idle;
        self.status_message = Some("Connected".into());
        Ok(())
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(runner) = &mut self.runner {
            runner.disconnect().await?;
        }
        self.runner = None;
        self.state = AppState::Idle;
        self.status_message = Some("Disconnected".into());
        Ok(())
    }

    pub async fn warmup(&mut self) -> Result<()> {
        self.state = AppState::Warming;
        self.status_message = Some("Warming up...".into());

        if let Some(runner) = &mut self.runner {
            runner.warmup().await?;
        }

        self.state = AppState::Idle;
        self.status_message = Some("Warmup complete".into());
        Ok(())
    }

    pub async fn run_burst(&mut self) -> Result<BurstMetrics> {
        self.state = AppState::Running;

        let runner = self
            .runner
            .as_mut()
            .ok_or_else(|| anyhow!("Not connected"))?;

        let metrics = runner.run_burst().await?;
        self.history.push(metrics.clone());
        self.state = AppState::Idle;
        Ok(metrics)
    }

    pub fn toggle_pause(&mut self) {
        self.state = match self.state {
            AppState::Running | AppState::Idle => AppState::Paused,
            AppState::Paused => AppState::Idle,
            _ => self.state,
        };
    }

    pub fn toggle_view(&mut self) {
        self.view = match self.view {
            View::Dashboard => View::Results,
            View::Results => View::Dashboard,
        };
    }

    /// Run a single sample query and store the results for display
    pub async fn run_sample(&mut self) -> Result<()> {
        let runner = self
            .runner
            .as_ref()
            .ok_or_else(|| anyhow!("Not connected"))?;

        let (query, results) = runner.run_sample_query().await.map_err(|e| anyhow!("{e}"))?;
        self.last_sample = Some(SampleResult { query, results });
        self.results_scroll = 0;
        Ok(())
    }

    pub fn start_editing(&mut self) {
        self.editing = true;
        self.query_input.clear();
    }

    pub fn cancel_editing(&mut self) {
        self.editing = false;
    }

    pub async fn submit_query(&mut self) -> Result<()> {
        let text = self.query_input.trim().to_string();
        if text.is_empty() {
            self.editing = false;
            return Ok(());
        }

        let embedder = self
            .embedder
            .as_ref()
            .ok_or_else(|| anyhow!("No embedder configured"))?;

        let mut embedded = embedder
            .embed_queries(&[text])
            .await
            .map_err(|e| anyhow!("{e}"))?;

        let eq = embedded
            .pop()
            .ok_or_else(|| anyhow!("Embedding returned no results"))?;

        let runner = self
            .runner
            .as_ref()
            .ok_or_else(|| anyhow!("Not connected"))?;

        let (query, results) = runner.run_custom_query(&eq).await.map_err(|e| anyhow!("{e}"))?;
        self.last_sample = Some(SampleResult { query, results });
        self.results_scroll = 0;
        self.editing = false;
        Ok(())
    }

    pub fn scroll_results(&mut self, delta: isize) {
        let max = self
            .last_sample
            .as_ref()
            .map(|s| s.results.results.len().saturating_sub(1))
            .unwrap_or(0);

        let current = self.results_scroll as isize;
        self.results_scroll = (current + delta).clamp(0, max as isize) as usize;
    }
}

fn create_provider(config: &ProviderConfig) -> Result<Box<dyn qstorm::SearchProvider>> {
    let name = config.name.clone();
    match &config.provider {
        #[cfg(feature = "elasticsearch")]
        ProviderKind::Elasticsearch(c) => Ok(Box::new(
            qstorm::providers::ElasticsearchProvider::new(name, c.clone()),
        )),

        #[cfg(feature = "qdrant")]
        ProviderKind::Qdrant(c) => Ok(Box::new(
            qstorm::providers::QdrantProvider::new(name, c.clone()),
        )),

        #[cfg(feature = "pgvector")]
        ProviderKind::Pgvector(c) => Ok(Box::new(
            qstorm::providers::PgvectorProvider::new(name, c.clone()),
        )),
    }
}