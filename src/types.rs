use serde::{Deserialize, Serialize};

/// A single search result from a provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Document identifier
    pub id: String,
    /// Relevance/similarity score
    pub score: f32,
    /// Optional payload/document content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

/// Collection of search results from a query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub results: Vec<SearchResult>,
    /// Time taken by the search engine (if reported), in milliseconds
    pub took_ms: Option<u64>,
    /// Total hits (may be more than returned results)
    pub total_hits: Option<u64>,
}

impl SearchResults {
    pub fn new(results: Vec<SearchResult>) -> Self {
        Self {
            results,
            took_ms: None,
            total_hits: None,
        }
    }

    pub fn with_took(mut self, took_ms: u64) -> Self {
        self.took_ms = Some(took_ms);
        self
    }

    pub fn with_total_hits(mut self, total: u64) -> Self {
        self.total_hits = Some(total);
        self
    }

    /// Get document IDs in order (for recall calculation)
    pub fn ids(&self) -> Vec<&str> {
        self.results.iter().map(|r| r.id.as_str()).collect()
    }
}

/// Parameters for search execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchParams {
    /// Number of results to return
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    /// Minimum score threshold (provider-specific interpretation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_score: Option<f32>,
    /// Request timeout in milliseconds
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    /// Include document payloads in results
    #[serde(default)]
    pub include_payload: bool,
}

fn default_top_k() -> usize {
    10
}

fn default_timeout() -> u64 {
    5000
}

impl Default for SearchParams {
    fn default() -> Self {
        Self {
            top_k: default_top_k(),
            min_score: None,
            timeout_ms: default_timeout(),
            include_payload: false,
        }
    }
}
