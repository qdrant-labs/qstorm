use async_trait::async_trait;
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    Document, Fusion, PointId, PrefetchQueryBuilder, Query, QueryPointsBuilder,
    SearchPointsBuilder,
};
use tracing::debug;

use crate::config::QdrantConfig;
use crate::error::{Error, Result};
use crate::provider::{Capabilities, SearchProvider};
use crate::types::{SearchParams, SearchResult, SearchResults};

pub struct QdrantProvider {
    name: String,
    config: QdrantConfig,
    client: Option<Qdrant>,
}

impl QdrantProvider {
    pub fn new(name: String, config: QdrantConfig) -> Self {
        Self {
            name,
            config,
            client: None,
        }
    }

    fn client(&self) -> Result<&Qdrant> {
        self.client.as_ref().ok_or(Error::NotConnected)
    }
}

#[async_trait]
impl SearchProvider for QdrantProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            vector_search: true,
            native_hybrid: self.config.text_field.is_some(),
            vector_dimension: None,
        }
    }

    async fn connect(&mut self) -> Result<()> {
        let mut builder = Qdrant::from_url(&self.config.url);

        if let Some(api_key) = &self.config.api_key {
            builder = builder.api_key(api_key.clone());
        }

        let client = builder
            .build()
            .map_err(|e| Error::Connection(e.to_string()))?;

        // Verify connection by checking collection exists
        let collections = client
            .list_collections()
            .await
            .map_err(|e| Error::Connection(e.to_string()))?;

        let exists = collections
            .collections
            .iter()
            .any(|c| c.name == self.config.collection_name);

        if !exists {
            return Err(Error::Config(format!(
                "Collection '{}' not found",
                self.config.collection_name
            )));
        }

        debug!(collection = %self.config.collection_name, "Connected to Qdrant");
        self.client = Some(client);
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.client = None;
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        let client = self.client()?;
        client
            .health_check()
            .await
            .map(|_| true)
            .map_err(|e| Error::Connection(e.to_string()))
    }

    async fn vector_search(&self, vector: &[f32], params: &SearchParams) -> Result<SearchResults> {
        let client = self.client()?;
        let vector_field = self.config.vector_field.as_deref();

        let mut search = SearchPointsBuilder::new(
            &self.config.collection_name,
            vector.to_vec(),
            params.top_k as u64,
        );

        if let Some(field) = vector_field {
            search = search.vector_name(field.to_string());
        }

        if params.include_payload {
            search = search.with_payload(true);
        }

        if let Some(min_score) = params.min_score {
            search = search.score_threshold(min_score);
        }

        // return payloads
        search = search.with_payload(true);

        let response = client
            .search_points(search)
            .await
            .map_err(|e| Error::QueryExecution(e.to_string()))?;

        let results: Vec<SearchResult> = response
            .result
            .into_iter()
            .map(|point| {
                let id = match point.id {
                    Some(PointId {
                        point_id_options: Some(id),
                    }) => {
                        use qdrant_client::qdrant::point_id::PointIdOptions;
                        match id {
                            PointIdOptions::Num(n) => n.to_string(),
                            PointIdOptions::Uuid(s) => s,
                        }
                    }
                    _ => "unknown".to_string(),
                };

                let payload = if params.include_payload {
                    Some(serde_json::to_value(&point.payload).unwrap_or_default())
                } else {
                    None
                };

                SearchResult {
                    id,
                    score: point.score,
                    payload,
                }
            })
            .collect();

        Ok(SearchResults::new(results))
    }

    async fn hybrid_search(
        &self,
        text: &str,
        vector: &[f32],
        params: &SearchParams,
    ) -> Result<SearchResults> {
        let client = self.client()?;

        let text_field = self.config.text_field.as_deref().ok_or_else(|| {
            Error::Config("Hybrid search requires 'text_field' to be set in provider config".into())
        })?;

        let limit = params.top_k as u64;
        let prefetch_limit = limit * 2;

        // BM25 prefetch: Qdrant tokenizes and scores server-side
        let bm25_prefetch = PrefetchQueryBuilder::default()
            .query(Query::new_nearest(Document::new(text, "qdrant/bm25")))
            .using(text_field.to_string())
            .limit(prefetch_limit);

        // Dense vector prefetch
        let mut dense_prefetch = PrefetchQueryBuilder::default()
            .query(Query::new_nearest(vector.to_vec()))
            .limit(prefetch_limit);

        if let Some(field) = self.config.vector_field.as_deref() {
            dense_prefetch = dense_prefetch.using(field.to_string());
        }

        // Fuse with RRF
        let query = QueryPointsBuilder::new(&self.config.collection_name)
            .add_prefetch(bm25_prefetch)
            .add_prefetch(dense_prefetch)
            .query(Fusion::Rrf)
            .with_payload(true)
            .limit(limit);

        let response = client
            .query(query)
            .await
            .map_err(|e| Error::QueryExecution(e.to_string()))?;

        let results: Vec<SearchResult> = response
            .result
            .into_iter()
            .map(|point| {
                let id = match point.id {
                    Some(PointId {
                        point_id_options: Some(id),
                    }) => {
                        use qdrant_client::qdrant::point_id::PointIdOptions;
                        match id {
                            PointIdOptions::Num(n) => n.to_string(),
                            PointIdOptions::Uuid(s) => s,
                        }
                    }
                    _ => "unknown".to_string(),
                };

                let payload = if params.include_payload {
                    Some(serde_json::to_value(&point.payload).unwrap_or_default())
                } else {
                    None
                };

                SearchResult {
                    id,
                    score: point.score,
                    payload,
                }
            })
            .collect();

        Ok(SearchResults::new(results))
    }
}
