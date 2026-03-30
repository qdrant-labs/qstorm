#[cfg(feature = "elasticsearch")]
pub mod elastic;

#[cfg(feature = "pgvector")]
pub mod pgvector;

#[cfg(feature = "qdrant")]
pub mod qdrant;

#[cfg(feature = "opensearch")]
pub mod opensearch;

// re-export provider types when features are enabled
#[cfg(feature = "elasticsearch")]
pub use elastic::ElasticsearchProvider;

#[cfg(feature = "pgvector")]
pub use pgvector::PgvectorProvider;

#[cfg(feature = "qdrant")]
pub use qdrant::QdrantProvider;

#[cfg(feature = "opensearch")]
pub use opensearch::OpenSearchProvider;