use async_openai::config::OpenAIConfig;
use async_openai::types::{CreateEmbeddingRequestArgs, EmbeddingInput};
use async_openai::Client as OpenAiClient;
use async_trait::async_trait;
use indicatif::{ProgressBar, ProgressStyle};
use tracing::{debug, info};

use super::EmbeddingProvider;
use crate::config::EmbeddingConfig;
use crate::error::{Error, Result};
use crate::queries::EmbeddedQuery;

/// OpenAI API-based embedding provider
pub struct OpenAIProvider {
    model: String,
    dimensions: u32,
    client: OpenAiClient<OpenAIConfig>,
}

impl OpenAIProvider {
    pub fn new(config: &EmbeddingConfig) -> Result<Self> {
        let api_key = config
            .api_key
            .clone()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .ok_or_else(|| {
                Error::Config(
                    "OpenAI API key required. Set 'api_key' in embedding config \
                     or OPENAI_API_KEY env var"
                        .into(),
                )
            })?;

        let dimensions = config.dimensions.unwrap_or(1536);

        let oai_config = OpenAIConfig::new().with_api_key(&api_key);
        let client = OpenAiClient::with_config(oai_config);

        Ok(Self {
            model: config.model.clone(),
            dimensions,
            client,
        })
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAIProvider {
    async fn embed_queries(&self, texts: &[String]) -> Result<Vec<EmbeddedQuery>> {
        info!(
            "Embedding {} queries with model={} dims={}",
            texts.len(),
            self.model,
            self.dimensions,
        );

        let mut queries = Vec::with_capacity(texts.len());
        let batch_size = 1024;
        let total_batches = texts.len().div_ceil(batch_size);

        let pb = ProgressBar::new(total_batches as u64);
        pb.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] \
                 {pos}/{len} batches ({msg})",
            )
            .unwrap()
            .progress_chars("#>-"),
        );
        pb.set_message("embedding...");

        for (batch_idx, batch) in texts.chunks(batch_size).enumerate() {
            debug!("Embedding batch of {} queries", batch.len());

            let mut builder = CreateEmbeddingRequestArgs::default();
            builder
                .model(&self.model)
                .input(EmbeddingInput::StringArray(batch.to_vec()))
                .dimensions(self.dimensions);

            let request = builder
                .build()
                .map_err(|e| Error::Config(format!("Failed to build embedding request: {e}")))?;

            let response = self
                .client
                .embeddings()
                .create(request)
                .await
                .map_err(|e| Error::Config(format!("OpenAI embedding request failed: {e}")))?;

            for (i, embedding) in response.data.iter().enumerate() {
                queries.push(EmbeddedQuery {
                    text: batch[i].clone(),
                    vector: embedding.embedding.to_vec(),
                });
            }

            pb.set_message(format!("{} embedded", queries.len()));
            pb.set_position((batch_idx + 1) as u64);
        }

        pb.finish_with_message(format!("{} queries embedded", queries.len()));
        info!("Embedded {} queries successfully", queries.len());
        Ok(queries)
    }

    fn dimension(&self) -> usize {
        self.dimensions as usize
    }
}