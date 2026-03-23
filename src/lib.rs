pub mod config;
pub mod embedder;
pub mod error;
pub mod metrics;
pub mod provider;
pub mod providers;
pub mod queries;
pub mod runner;
pub mod types;

// re-exports
pub use config::{Config, SearchMode};
pub use embedder::{Embedder, EmbeddingProvider};
pub use error::{Error, Result};
pub use metrics::{BurstMetrics, Metrics};
pub use provider::{Capabilities, SearchProvider};
pub use queries::{EmbeddedQuery, QueryFile};
pub use runner::BenchmarkRunner;
pub use types::{SearchParams, SearchResult, SearchResults};
