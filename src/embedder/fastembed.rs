use ::fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use async_trait::async_trait;

use super::EmbeddingProvider;
use crate::error::{Error, Result};
use crate::queries::EmbeddedQuery;

/// Fastembed-based local embedding provider
pub struct FastEmbedProvider {
    model: TextEmbedding,
}

impl FastEmbedProvider {
    pub fn new(model_name: &str) -> Result<Self> {
        let model = parse_model(model_name)?;
        let embedding =
            TextEmbedding::try_new(InitOptions::new(model).with_show_download_progress(true))
                .map_err(|e| Error::Config(format!("Failed to load embedding model: {}", e)))?;
        Ok(Self { model: embedding })
    }
}

#[async_trait]
impl EmbeddingProvider for FastEmbedProvider {
    async fn embed_queries(&self, texts: &[String]) -> Result<Vec<EmbeddedQuery>> {
        let embeddings = self
            .model
            .embed(texts.to_vec(), None)
            .map_err(|e| Error::Config(format!("Embedding failed: {}", e)))?;

        let queries = texts
            .iter()
            .zip(embeddings)
            .map(|(text, vector)| EmbeddedQuery {
                text: text.clone(),
                vector,
            })
            .collect();

        Ok(queries)
    }

    fn dimension(&self) -> usize {
        self.model
            .embed(vec!["test"], None)
            .map(|v| v.first().map(|e| e.len()).unwrap_or(0))
            .unwrap_or(0)
    }
}

fn parse_model(name: &str) -> Result<EmbeddingModel> {
    match name {
        "BAAI/bge-small-en-v1.5" | "bge-small-en-v1.5" => Ok(EmbeddingModel::BGESmallENV15),
        "BAAI/bge-base-en-v1.5" | "bge-base-en-v1.5" => Ok(EmbeddingModel::BGEBaseENV15),
        "BAAI/bge-large-en-v1.5" | "bge-large-en-v1.5" => Ok(EmbeddingModel::BGELargeENV15),
        "sentence-transformers/all-MiniLM-L6-v2" | "all-MiniLM-L6-v2" => {
            Ok(EmbeddingModel::AllMiniLML6V2)
        }
        "sentence-transformers/all-MiniLM-L12-v2" | "all-MiniLM-L12-v2" => {
            Ok(EmbeddingModel::AllMiniLML12V2)
        }
        _ => Err(Error::Config(format!(
            "Unknown embedding model: {}. Supported: bge-small-en-v1.5, bge-base-en-v1.5, \
             bge-large-en-v1.5, all-MiniLM-L6-v2, all-MiniLM-L12-v2",
            name
        ))),
    }
}