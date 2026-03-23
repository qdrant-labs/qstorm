#[cfg(feature = "elasticsearch")]
pub mod elastic;

#[cfg(feature = "pgvector")]
pub mod pgvector;

#[cfg(feature = "qdrant")]
pub mod qdrant;

// re-export provider types when features are enabled
#[cfg(feature = "elasticsearch")]
pub use elastic::ElasticsearchProvider;

#[cfg(feature = "pgvector")]
pub use pgvector::PgvectorProvider;

#[cfg(feature = "qdrant")]
pub use qdrant::QdrantProvider;
