use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Query file format - simple list of text queries to embed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryFile {
    /// Text queries to embed and search with
    pub queries: Vec<String>,
}

impl QueryFile {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let query_file: QueryFile = serde_yaml::from_str(&contents)?;
        Ok(query_file)
    }

    pub fn from_str(yaml: &str) -> Result<Self> {
        let query_file: QueryFile = serde_yaml::from_str(yaml)?;
        Ok(query_file)
    }
}

/// Embedded query ready for vector search
#[derive(Debug, Clone)]
pub struct EmbeddedQuery {
    /// Original text
    pub text: String,
    /// Embedding vector
    pub vector: Vec<f32>,
}
