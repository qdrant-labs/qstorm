#[cfg(feature = "embeddings")]
mod fastembed;
#[cfg(feature = "openai-embeddings")]
mod openai;

#[cfg(feature = "embeddings")]
pub use fastembed::FastEmbedProvider;
#[cfg(feature = "openai-embeddings")]
pub use openai::OpenAIProvider;

use async_trait::async_trait;

use crate::config::EmbeddingConfig;
use crate::error::Result;
use crate::queries::EmbeddedQuery;

/// Trait for embedding text into vectors
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a batch of text queries into vectors
    async fn embed_queries(&self, texts: &[String]) -> Result<Vec<EmbeddedQuery>>;

    /// Get the embedding dimension for this provider
    fn dimension(&self) -> usize;
}

/// Unified embedder dispatching to the configured backend
pub enum Embedder {
    #[cfg(feature = "embeddings")]
    FastEmbed(FastEmbedProvider),
    #[cfg(feature = "openai-embeddings")]
    OpenAI(OpenAIProvider),
    #[cfg(not(any(feature = "embeddings", feature = "openai-embeddings")))]
    #[doc(hidden)]
    _Disabled(std::convert::Infallible),
}

impl Embedder {
    /// Create an embedder from configuration.
    ///
    /// Models namespaced with `openai/` (e.g. `openai/text-embedding-3-small`)
    /// dispatch to OpenAI; all others dispatch to fastembed.
    pub fn from_config(config: &EmbeddingConfig) -> Result<Self> {
        if let Some(model) = config.model.strip_prefix("openai/") {
            let mut config = config.clone();
            config.model = model.to_owned();
            Self::new_openai(&config)
        } else {
            Self::new_fastembed(config)
        }
    }

    #[cfg(feature = "openai-embeddings")]
    fn new_openai(config: &EmbeddingConfig) -> Result<Self> {
        Ok(Self::OpenAI(OpenAIProvider::new(config)?))
    }

    #[cfg(not(feature = "openai-embeddings"))]
    fn new_openai(config: &EmbeddingConfig) -> Result<Self> {
        Err(crate::error::Error::Config(format!(
            "Model '{}' requires the 'openai-embeddings' feature. \
             Rebuild with --features openai-embeddings",
            config.model
        )))
    }

    #[cfg(feature = "embeddings")]
    fn new_fastembed(config: &EmbeddingConfig) -> Result<Self> {
        Ok(Self::FastEmbed(FastEmbedProvider::new(&config.model)?))
    }

    #[cfg(not(feature = "embeddings"))]
    fn new_fastembed(config: &EmbeddingConfig) -> Result<Self> {
        Err(crate::error::Error::Config(format!(
            "Model '{}' requires the 'embeddings' feature. \
             Rebuild with --features embeddings",
            config.model
        )))
    }

    /// Embed a batch of text queries
    #[allow(unused_variables)]
    pub async fn embed_queries(&self, texts: &[String]) -> Result<Vec<EmbeddedQuery>> {
        match self {
            #[cfg(feature = "embeddings")]
            Self::FastEmbed(p) => p.embed_queries(texts).await,
            #[cfg(feature = "openai-embeddings")]
            Self::OpenAI(p) => p.embed_queries(texts).await,
            #[cfg(not(any(feature = "embeddings", feature = "openai-embeddings")))]
            Self::_Disabled(never) => match *never {},
        }
    }

    /// Get the embedding dimension
    pub fn dimension(&self) -> usize {
        match self {
            #[cfg(feature = "embeddings")]
            Self::FastEmbed(p) => p.dimension(),
            #[cfg(feature = "openai-embeddings")]
            Self::OpenAI(p) => p.dimension(),
            #[cfg(not(any(feature = "embeddings", feature = "openai-embeddings")))]
            Self::_Disabled(never) => match *never {},
        }
    }
}
