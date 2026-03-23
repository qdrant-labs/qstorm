use async_trait::async_trait;
use pgvector::Vector;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use tracing::debug;

use crate::config::PgvectorConfig;
use crate::error::{Error, Result};
use crate::provider::{Capabilities, SearchProvider};
use crate::types::{SearchParams, SearchResult, SearchResults};

pub struct PgvectorProvider {
    name: String,
    config: PgvectorConfig,
    pool: Option<PgPool>,
}

impl PgvectorProvider {
    pub fn new(name: String, config: PgvectorConfig) -> Self {
        Self {
            name,
            config,
            pool: None,
        }
    }

    fn pool(&self) -> Result<&PgPool> {
        self.pool.as_ref().ok_or(Error::NotConnected)
    }
}

#[async_trait]
impl SearchProvider for PgvectorProvider {
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
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&self.config.url)
            .await
            .map_err(|e| Error::Connection(e.to_string()))?;

        // Verify table exists
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = $1)",
        )
        .bind(&self.config.table_name)
        .fetch_one(&pool)
        .await
        .map_err(|e| Error::Connection(e.to_string()))?;

        if !exists {
            return Err(Error::Config(format!(
                "Table '{}' not found",
                self.config.table_name
            )));
        }

        debug!(table = %self.config.table_name, "Connected to pgvector");
        self.pool = Some(pool);
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(pool) = self.pool.take() {
            pool.close().await;
        }
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        let pool = self.pool()?;
        sqlx::query("SELECT 1")
            .execute(pool)
            .await
            .map(|_| true)
            .map_err(|e| Error::Connection(e.to_string()))
    }

    async fn vector_search(&self, vector: &[f32], params: &SearchParams) -> Result<SearchResults> {
        let pool = self.pool()?;
        let vector_field = self.config.vector_field.as_deref().unwrap_or("embedding");
        let table = &self.config.table_name;
        let embedding = Vector::from(vector.to_vec());

        let query = if params.include_payload {
            format!(
                "SELECT id::text, 1 - ({vector_field} <=> $1::vector) as score, \
                 to_jsonb(t) - '{vector_field}' - 'id' as payload \
                 FROM {table} t \
                 ORDER BY {vector_field} <=> $1::vector \
                 LIMIT $2"
            )
        } else {
            format!(
                "SELECT id::text, 1 - ({vector_field} <=> $1::vector) as score \
                 FROM {table} \
                 ORDER BY {vector_field} <=> $1::vector \
                 LIMIT $2"
            )
        };

        let rows = sqlx::query(&query)
            .bind(&embedding)
            .bind(params.top_k as i64)
            .fetch_all(pool)
            .await
            .map_err(|e| Error::QueryExecution(e.to_string()))?;

        let results: Vec<SearchResult> = rows
            .iter()
            .filter_map(|row| {
                let id: String = row.try_get("id").ok()?;
                let score: f64 = row.try_get("score").ok()?;
                let payload = if params.include_payload {
                    row.try_get::<serde_json::Value, _>("payload").ok()
                } else {
                    None
                };
                Some(SearchResult {
                    id,
                    score: score as f32,
                    payload,
                })
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
        let pool = self.pool()?;

        let text_field = self.config.text_field.as_deref().ok_or_else(|| {
            Error::Config(
                "Hybrid search requires 'text_field' to be set in provider config".into(),
            )
        })?;
        let vector_field = self.config.vector_field.as_deref().unwrap_or("embedding");
        let table = &self.config.table_name;
        let embedding = Vector::from(vector.to_vec());
        let prefetch_limit = (params.top_k * 2) as i64;
        let limit = params.top_k as i64;

        let query = format!(
            "WITH vector_results AS ( \
                SELECT id::text, ROW_NUMBER() OVER (ORDER BY {vector_field} <=> $1::vector) as rank \
                FROM {table} \
                ORDER BY {vector_field} <=> $1::vector \
                LIMIT $3 \
            ), \
            text_results AS ( \
                SELECT id::text, ROW_NUMBER() OVER ( \
                    ORDER BY ts_rank(to_tsvector('english', {text_field}), plainto_tsquery('english', $2)) DESC \
                ) as rank \
                FROM {table} \
                WHERE to_tsvector('english', {text_field}) @@ plainto_tsquery('english', $2) \
                LIMIT $3 \
            ) \
            SELECT COALESCE(v.id, t.id) as id, \
                   COALESCE(1.0 / (60 + v.rank), 0) + COALESCE(1.0 / (60 + t.rank), 0) as score \
            FROM vector_results v \
            FULL OUTER JOIN text_results t ON v.id = t.id \
            ORDER BY score DESC \
            LIMIT $4"
        );

        let rows = sqlx::query(&query)
            .bind(&embedding)
            .bind(text)
            .bind(prefetch_limit)
            .bind(limit)
            .fetch_all(pool)
            .await
            .map_err(|e| Error::QueryExecution(e.to_string()))?;

        let results: Vec<SearchResult> = rows
            .iter()
            .filter_map(|row| {
                let id: String = row.try_get("id").ok()?;
                let score: f64 = row.try_get("score").ok()?;
                Some(SearchResult {
                    id,
                    score: score as f32,
                    payload: None,
                })
            })
            .collect();

        Ok(SearchResults::new(results))
    }
}
