use async_trait::async_trait;
use opensearch::{
    OpenSearch, SearchParts, auth::Credentials, cert::CertificateValidation, http::transport::{SingleNodeConnectionPool, TransportBuilder}
};
use serde_json::json;
use tracing::debug;

use crate::config::{OpenSearchConfig, OpenSearchCredentials};
use crate::error::{Error, Result};
use crate::provider::{Capabilities, SearchProvider};
use crate::types::{SearchParams, SearchResult, SearchResults};

pub struct OpenSearchProvider {
    name: String,
    config: OpenSearchConfig,
    client: Option<OpenSearch>,
}

impl OpenSearchProvider {
    pub fn new(name: String, config: OpenSearchConfig) -> Self {
        Self {
            name,
            config,
            client: None,
        }
    }

    fn client(&self) -> Result<&OpenSearch> {
        self.client.as_ref().ok_or(Error::NotConnected)
    }
}

#[async_trait]
impl SearchProvider for OpenSearchProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            vector_search: true,
            keyword_search: true,
            native_hybrid: true,
            vector_dimension: None,
        }
    }

    async fn connect(&mut self) -> Result<()> {
        let url = self
            .config
            .url
            .parse()
            .map_err(|e| Error::Config(format!("Invalid URL: {}", e)))?;

        let pool = SingleNodeConnectionPool::new(url);
        let mut builder = TransportBuilder::new(pool);

        if let Some(creds) = &self.config.credentials {
            builder = match creds {
                OpenSearchCredentials::Basic { username, password } => {
                    builder.auth(Credentials::Basic(username.clone(), password.clone()))
                }
                OpenSearchCredentials::ApiKey { key } => {
                    builder.auth(Credentials::ApiKey(key.clone(), "".to_string()))
                }
                OpenSearchCredentials::Bearer { token } => {
                    builder.auth(Credentials::Bearer(token.clone()))
                }
            };
        }

        let cert_validation = if self.config.skip_cert {
            CertificateValidation::None
        } else {
            CertificateValidation::Default
        };

        let transport = builder
            .cert_validation(cert_validation)
            .build()
            .map_err(|e| Error::Connection(e.to_string()))?;

        let client = OpenSearch::new(transport);

        // Verify connection
        let response = client
            .cat()
            .health()
            .send()
            .await
            .map_err(|e| Error::Connection(e.to_string()))?;

        if !response.status_code().is_success() {
            return Err(Error::Connection("Health check failed".into()));
        }

        debug!(index = %self.config.index_name, "Connected to OpenSearch");
        self.client = Some(client);
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.client = None;
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        let client = self.client()?;
        let response = client
            .cat()
            .health()
            .send()
            .await
            .map_err(|e| Error::Connection(e.to_string()))?;
        Ok(response.status_code().is_success())
    }

    async fn vector_search(&self, vector: &[f32], params: &SearchParams) -> Result<SearchResults> {
        let client = self.client()?;
        let vector_field = self.config.vector_field.as_deref().unwrap_or("vector");

        let body = json!({
            "size": params.top_k,
            "query": {
                "knn": {
                    vector_field: {
                        "vector": vector,
                        "k": params.top_k
                    }
                }
            }
        });

        let response = client
            .search(SearchParts::Index(&[&self.config.index_name]))
            .body(body)
            .send()
            .await
            .map_err(|e| Error::QueryExecution(e.to_string()))?;

        if !response.status_code().is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(Error::QueryExecution(format!(
                "Search failed: {}",
                error_body
            )));
        }

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::InvalidResponse(e.to_string()))?;

        let took_ms = response_body["took"].as_u64();
        let total_hits = response_body["hits"]["total"]["value"].as_u64();

        let hits = response_body["hits"]["hits"]
            .as_array()
            .ok_or_else(|| Error::InvalidResponse("Missing hits array".into()))?;

        let results: Vec<SearchResult> = hits
            .iter()
            .filter_map(|hit| {
                let id = hit["_id"].as_str()?.to_string();
                let score = hit["_score"].as_f64().unwrap_or(0.0) as f32;
                let payload = if params.include_payload {
                    hit.get("_source").cloned()
                } else {
                    None
                };
                Some(SearchResult { id, score, payload })
            })
            .collect();

        let mut search_results = SearchResults::new(results);
        if let Some(took) = took_ms {
            search_results = search_results.with_took(took);
        }
        if let Some(total) = total_hits {
            search_results = search_results.with_total_hits(total);
        }

        Ok(search_results)
    }

    async fn keyword_search(
        &self,
        text: &str,
        params: &SearchParams,
    ) -> Result<SearchResults> {
        let client = self.client()?;
        let text_field = self.config.text_field.as_deref().unwrap_or("text");

        let body = json!({
            "size": params.top_k,
            "query": {
                "match": {
                    text_field: text
                }
            }
        });

        let response = client
            .search(SearchParts::Index(&[&self.config.index_name]))
            .body(body)
            .send()
            .await
            .map_err(|e| Error::QueryExecution(e.to_string()))?;

        if !response.status_code().is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(Error::QueryExecution(format!(
                "Keyword search failed: {}",
                error_body
            )));
        }

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::InvalidResponse(e.to_string()))?;

        let took_ms = response_body["took"].as_u64();
        let total_hits = response_body["hits"]["total"]["value"].as_u64();

        let hits = response_body["hits"]["hits"]
            .as_array()
            .ok_or_else(|| Error::InvalidResponse("Missing hits array".into()))?;

        let results: Vec<SearchResult> = hits
            .iter()
            .filter_map(|hit| {
                let id = hit["_id"].as_str()?.to_string();
                let score = hit["_score"].as_f64().unwrap_or(0.0) as f32;
                let payload = if params.include_payload {
                    hit.get("_source").cloned()
                } else {
                    None
                };
                Some(SearchResult { id, score, payload })
            })
            .collect();

        let mut search_results = SearchResults::new(results);
        if let Some(took) = took_ms {
            search_results = search_results.with_took(took);
        }
        if let Some(total) = total_hits {
            search_results = search_results.with_total_hits(total);
        }

        Ok(search_results)
    }

async fn hybrid_search(
        &self,
        text: &str,
        vector: &[f32],
        params: &SearchParams,
    ) -> Result<SearchResults> {
        let client = self.client()?;
        let text_field = self.config.text_field.as_deref().unwrap_or("text");
        let vector_field = self.config.vector_field.as_deref().unwrap_or("vector");

        // The correct OpenSearch Hybrid Syntax
        let body = json!({
            "size": params.top_k,
            "query": {
                "hybrid": {
                    "queries": [
                        {
                            "match": {
                                text_field: text
                            }
                        },
                        {
                            "knn": {
                                vector_field: {
                                    "vector": vector,
                                    "k": params.top_k
                                }
                            }
                        }
                    ]
                }
            }
        });

        let response = client
            .search(SearchParts::Index(&[&self.config.index_name]))
            .body(body)
            .send()
            .await
            .map_err(|e| Error::QueryExecution(e.to_string()))?;

        if !response.status_code().is_success() {
            let error_body = response.text().await.unwrap_or_default();
            
            return Err(Error::QueryExecution(format!(
                "Hybrid search failed: {}",
                error_body
            )));
        }

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::InvalidResponse(e.to_string()))?;

        let took_ms = response_body["took"].as_u64();
        let total_hits = response_body["hits"]["total"]["value"].as_u64();

        let hits = response_body["hits"]["hits"]
            .as_array()
            .ok_or_else(|| Error::InvalidResponse("Missing hits array".into()))?;

        let results: Vec<SearchResult> = hits
            .iter()
            .filter_map(|hit| {
                let id = hit["_id"].as_str()?.to_string();
                let score = hit["_score"].as_f64().unwrap_or(0.0) as f32;
                let payload = if params.include_payload {
                    hit.get("_source").cloned()
                } else {
                    None
                };
                Some(SearchResult { id, score, payload })
            })
            .collect();

        let mut search_results = SearchResults::new(results);
        if let Some(took) = took_ms {
            search_results = search_results.with_took(took);
        }
        if let Some(total) = total_hits {
            search_results = search_results.with_total_hits(total);
        }

        Ok(search_results)
    }
}
