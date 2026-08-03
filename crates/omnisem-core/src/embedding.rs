//! Embedding provider contracts, deterministic spaces, and vector encoding.
#![allow(
    clippy::cast_possible_truncation,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::map_unwrap_or,
    clippy::unused_self,
    clippy::wildcard_imports
)]

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::config::EmbeddingConfig;
use crate::domain::ContentHash;
use crate::hash::blake3_hex;

/// Versioned text-to-provider input contract.
pub const EMBEDDING_INPUT_CONTRACT_VERSION: &str = "segment-text-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingProviderKind {
    None,
    Ollama,
}

impl fmt::Display for EmbeddingProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::None => "none",
            Self::Ollama => "ollama",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEmbeddingModel {
    pub provider: EmbeddingProviderKind,
    pub configured_name: String,
    pub canonical_name: String,
    pub model_digest: String,
    pub dimensions: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorNormalization {
    L2,
}

impl fmt::Display for VectorNormalization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("l2")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingSpace {
    pub id: String,
    pub provider: EmbeddingProviderKind,
    pub canonical_model: String,
    pub model_digest: String,
    pub dimensions: u32,
    pub normalization: VectorNormalization,
    pub input_contract_version: String,
}

impl EmbeddingSpace {
    #[must_use]
    pub fn new(model: &ResolvedEmbeddingModel, dimensions: u32) -> Self {
        let normalization = VectorNormalization::L2;
        let payload = format!(
            "provider={}\nmodel={}\ndigest={}\ndimensions={}\nnormalization={}\ninput_contract={}\n",
            model.provider,
            model.canonical_name,
            model.model_digest,
            dimensions,
            normalization,
            EMBEDDING_INPUT_CONTRACT_VERSION
        );
        Self {
            id: format!("es_{}", blake3_hex(payload.as_bytes()).0),
            provider: model.provider,
            canonical_model: model.canonical_name.clone(),
            model_digest: model.model_digest.clone(),
            dimensions,
            normalization,
            input_contract_version: EMBEDDING_INPUT_CONTRACT_VERSION.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EmbeddingInput {
    pub text_hash: ContentHash,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingVector {
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingBatch {
    pub vectors: Vec<EmbeddingVector>,
}

pub trait EmbeddingProvider: Send + Sync {
    fn provider_kind(&self) -> EmbeddingProviderKind;
    fn resolve_model(&self) -> Result<ResolvedEmbeddingModel, EmbeddingError>;
    fn embed(
        &self,
        inputs: &[EmbeddingInput],
        model: &ResolvedEmbeddingModel,
    ) -> Result<EmbeddingBatch, EmbeddingError>;
}

/// Resolves the explicitly configured provider for diagnostics without embedding input.
pub fn diagnose_provider(
    config: &EmbeddingConfig,
) -> Result<Option<ResolvedEmbeddingModel>, EmbeddingError> {
    if !config.enabled {
        return Ok(None);
    }
    #[cfg(feature = "embeddings-ollama")]
    {
        let provider = ollama::OllamaProvider::new(config)?;
        provider.resolve_model().map(Some)
    }
    #[cfg(not(feature = "embeddings-ollama"))]
    {
        let _ = config;
        Err(EmbeddingError::FeatureDisabled)
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum EmbeddingError {
    #[error("EMBEDDING_UNAVAILABLE: embeddings are disabled")]
    Unavailable,
    #[error("EMBEDDING_FEATURE_DISABLED: this build does not include Ollama support")]
    FeatureDisabled,
    #[error("EMBEDDING_MODEL_NOT_FOUND: configured model was not reported by Ollama")]
    ModelNotFound,
    #[error("EMBEDDING_FAILED: {0}")]
    Failed(String),
    #[error("EMBEDDING_RESPONSE_INVALID: {0}")]
    ResponseInvalid(String),
    #[error("EMBEDDING_DIMENSION_MISMATCH: expected {expected}, received {actual}")]
    DimensionMismatch { expected: u32, actual: u32 },
    #[error("EMBEDDING_VECTOR_INVALID: {0}")]
    VectorInvalid(String),
    #[error("EMBEDDING_CACHE_CORRUPT: {0}")]
    CacheCorrupt(String),
}

impl EmbeddingError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable => "EMBEDDING_UNAVAILABLE",
            Self::FeatureDisabled => "EMBEDDING_FEATURE_DISABLED",
            Self::ModelNotFound => "EMBEDDING_MODEL_NOT_FOUND",
            Self::Failed(_) => "EMBEDDING_FAILED",
            Self::ResponseInvalid(_) => "EMBEDDING_RESPONSE_INVALID",
            Self::DimensionMismatch { .. } => "EMBEDDING_DIMENSION_MISMATCH",
            Self::VectorInvalid(_) => "EMBEDDING_VECTOR_INVALID",
            Self::CacheCorrupt(_) => "EMBEDDING_CACHE_CORRUPT",
        }
    }
    #[must_use]
    pub fn systemic(&self) -> bool {
        matches!(
            self,
            Self::Unavailable | Self::FeatureDisabled | Self::ModelNotFound | Self::Failed(_)
        )
    }
}

pub struct DisabledEmbeddingProvider;
impl EmbeddingProvider for DisabledEmbeddingProvider {
    fn provider_kind(&self) -> EmbeddingProviderKind {
        EmbeddingProviderKind::None
    }
    fn resolve_model(&self) -> Result<ResolvedEmbeddingModel, EmbeddingError> {
        Err(EmbeddingError::Unavailable)
    }
    fn embed(
        &self,
        _: &[EmbeddingInput],
        _: &ResolvedEmbeddingModel,
    ) -> Result<EmbeddingBatch, EmbeddingError> {
        Err(EmbeddingError::Unavailable)
    }
}

/// Validates and L2-normalizes provider output.
pub fn normalize_vector(values: &[f32]) -> Result<EmbeddingVector, EmbeddingError> {
    if values.is_empty() {
        return Err(EmbeddingError::VectorInvalid("empty vector".into()));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(EmbeddingError::VectorInvalid("non-finite component".into()));
    }
    let norm_sq = values
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>();
    if !norm_sq.is_finite() || norm_sq <= f64::EPSILON {
        return Err(EmbeddingError::VectorInvalid("zero or invalid norm".into()));
    }
    let norm = norm_sq.sqrt();
    let normalized = values
        .iter()
        .map(|value| (f64::from(*value) / norm) as f32)
        .collect::<Vec<_>>();
    if normalized.iter().any(|value| !value.is_finite()) {
        return Err(EmbeddingError::VectorInvalid(
            "normalization produced non-finite data".into(),
        ));
    }
    Ok(EmbeddingVector { values: normalized })
}

/// Validates that persisted values are already finite, nonzero, and L2 normalized.
pub fn validate_normalized_vector(values: &[f32]) -> Result<(), EmbeddingError> {
    if values.is_empty() {
        return Err(EmbeddingError::CacheCorrupt("empty vector".into()));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(EmbeddingError::CacheCorrupt(
            "vector contains a non-finite component".into(),
        ));
    }
    let norm_sq = values
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>();
    if !norm_sq.is_finite() || norm_sq <= f64::EPSILON {
        return Err(EmbeddingError::CacheCorrupt(
            "vector has zero or invalid norm".into(),
        ));
    }
    if (norm_sq - 1.0).abs() > 1.0e-4 {
        return Err(EmbeddingError::CacheCorrupt(
            "vector is not L2 normalized".into(),
        ));
    }
    Ok(())
}

/// Encodes normalized f32 values in deterministic little-endian order.
pub fn encode_vector(vector: &EmbeddingVector, dimensions: u32) -> Result<Vec<u8>, EmbeddingError> {
    if vector.values.len() != dimensions as usize {
        return Err(EmbeddingError::DimensionMismatch {
            expected: dimensions,
            actual: vector.values.len() as u32,
        });
    }
    let normalized = normalize_vector(&vector.values)?;
    let mut bytes = Vec::with_capacity(normalized.values.len() * 4);
    for value in normalized.values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Ok(bytes)
}

/// Decodes and validates deterministic little-endian vector bytes.
pub fn decode_vector(bytes: &[u8], dimensions: u32) -> Result<EmbeddingVector, EmbeddingError> {
    let expected = dimensions as usize * 4;
    if bytes.len() != expected {
        return Err(EmbeddingError::CacheCorrupt(format!(
            "expected {expected} vector bytes, found {}",
            bytes.len()
        )));
    }
    let values = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().expect("four-byte chunk")))
        .collect::<Vec<_>>();
    validate_normalized_vector(&values)?;
    Ok(EmbeddingVector { values })
}

#[cfg(feature = "embeddings-ollama")]
pub mod ollama {
    use super::*;
    use std::time::Duration;

    const MAX_RESPONSE_BYTES: u64 = 32 * 1024 * 1024;

    pub struct OllamaProvider {
        config: EmbeddingConfig,
        agent: ureq::Agent,
    }

    impl OllamaProvider {
        pub fn new(config: &EmbeddingConfig) -> Result<Self, EmbeddingError> {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(config.request_timeout_seconds)))
                .timeout_connect(Some(Duration::from_secs(
                    config.request_timeout_seconds.min(10),
                )))
                .max_redirects(0)
                .proxy(None)
                .build()
                .into();
            Ok(Self {
                config: config.clone(),
                agent,
            })
        }
        fn url(&self, path: &str) -> Result<String, EmbeddingError> {
            let mut url = url::Url::parse(&self.config.endpoint)
                .map_err(|_| EmbeddingError::Failed("configured endpoint is invalid".into()))?;
            url.set_path(path);
            url.set_query(None);
            url.set_fragment(None);
            Ok(url.into())
        }
        fn read_json<T: serde::de::DeserializeOwned>(
            &self,
            response: ureq::http::Response<ureq::Body>,
        ) -> Result<T, EmbeddingError> {
            if !response.status().is_success() {
                return Err(EmbeddingError::Failed(format!(
                    "Ollama returned HTTP {}",
                    response.status().as_u16()
                )));
            }
            let mut body = response.into_body();
            let bytes = body
                .with_config()
                .limit(MAX_RESPONSE_BYTES)
                .read_to_vec()
                .map_err(|error| {
                    EmbeddingError::ResponseInvalid(format!(
                        "bounded response read failed: {error}"
                    ))
                })?;
            serde_json::from_slice(&bytes)
                .map_err(|_| EmbeddingError::ResponseInvalid("malformed JSON response".into()))
        }
    }

    #[derive(Deserialize)]
    struct Tags {
        models: Vec<TagModel>,
    }
    #[derive(Deserialize)]
    struct TagModel {
        name: String,
        #[serde(default)]
        model: String,
        digest: String,
    }
    #[derive(Deserialize)]
    struct EmbedResponse {
        #[serde(default)]
        model: String,
        embeddings: Vec<Vec<f32>>,
    }

    impl EmbeddingProvider for OllamaProvider {
        fn provider_kind(&self) -> EmbeddingProviderKind {
            EmbeddingProviderKind::Ollama
        }
        fn resolve_model(&self) -> Result<ResolvedEmbeddingModel, EmbeddingError> {
            let response = self
                .agent
                .get(self.url("/api/tags")?)
                .call()
                .map_err(|error| {
                    EmbeddingError::Failed(format!("Ollama model discovery failed: {error}"))
                })?;
            let tags: Tags = self.read_json(response)?;
            let configured = self.config.model.as_str();
            let implicit_latest = if configured.contains(':') {
                configured.to_owned()
            } else {
                format!("{configured}:latest")
            };
            let found = tags
                .models
                .into_iter()
                .find(|item| {
                    item.name == configured
                        || item.model == configured
                        || item.name == implicit_latest
                        || item.model == implicit_latest
                })
                .ok_or(EmbeddingError::ModelNotFound)?;
            if found.digest.trim().is_empty() {
                return Err(EmbeddingError::ResponseInvalid(
                    "model digest is empty".into(),
                ));
            }
            let canonical = if found.model.is_empty() {
                found.name
            } else {
                found.model
            };
            Ok(ResolvedEmbeddingModel {
                provider: EmbeddingProviderKind::Ollama,
                configured_name: configured.into(),
                canonical_name: canonical,
                model_digest: found.digest,
                dimensions: (self.config.dimensions != 0).then_some(self.config.dimensions),
            })
        }
        fn embed(
            &self,
            inputs: &[EmbeddingInput],
            model: &ResolvedEmbeddingModel,
        ) -> Result<EmbeddingBatch, EmbeddingError> {
            if inputs.is_empty() || inputs.len() > self.config.batch_size {
                return Err(EmbeddingError::Failed(
                    "embedding batch is empty or exceeds configured batch size".into(),
                ));
            }
            let mut request = serde_json::json!({"model": self.config.model, "input": inputs.iter().map(|item| &item.text).collect::<Vec<_>>(), "truncate": false});
            if !self.config.keep_alive.is_empty() {
                request["keep_alive"] = self.config.keep_alive.clone().into();
            }
            if self.config.dimensions != 0 {
                request["dimensions"] = self.config.dimensions.into();
            }
            let body = serde_json::to_vec(&request)
                .map_err(|_| EmbeddingError::Failed("request serialization failed".into()))?;
            let response = self
                .agent
                .post(self.url("/api/embed")?)
                .header("content-type", "application/json")
                .send(&body)
                .map_err(|error| {
                    EmbeddingError::Failed(format!("Ollama embedding request failed: {error}"))
                })?;
            let response: EmbedResponse = self.read_json(response)?;
            if !response.model.is_empty()
                && response.model != model.configured_name
                && response.model != model.canonical_name
            {
                return Err(EmbeddingError::ResponseInvalid(
                    "returned model is incompatible with resolved model".into(),
                ));
            }
            if response.embeddings.len() != inputs.len() {
                return Err(EmbeddingError::ResponseInvalid(
                    "vector count differs from input count".into(),
                ));
            }
            let dimensions = response.embeddings.first().map(Vec::len).unwrap_or(0) as u32;
            if dimensions == 0
                || response
                    .embeddings
                    .iter()
                    .any(|values| values.len() as u32 != dimensions)
            {
                return Err(EmbeddingError::ResponseInvalid(
                    "vectors are empty or dimensions are inconsistent".into(),
                ));
            }
            if let Some(expected) = model.dimensions
                && expected != dimensions
            {
                return Err(EmbeddingError::DimensionMismatch {
                    expected,
                    actual: dimensions,
                });
            }
            let vectors = response
                .embeddings
                .iter()
                .map(|values| normalize_vector(values))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(EmbeddingBatch { vectors })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "embeddings-ollama")]
    struct MockReply {
        status: u16,
        body: Vec<u8>,
        delay_ms: u64,
    }

    #[cfg(feature = "embeddings-ollama")]
    fn mock_server(
        replies: Vec<MockReply>,
    ) -> (
        std::net::SocketAddr,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        std::thread::JoinHandle<()>,
    ) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::{Arc, Mutex};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let server = std::thread::spawn(move || {
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let read = stream.read(&mut buffer).unwrap();
                    if read == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..read]);
                    let Some(end) = bytes
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .map(|p| p + 4)
                    else {
                        continue;
                    };
                    let header = String::from_utf8_lossy(&bytes[..end]);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + length {
                        break;
                    }
                }
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&bytes).into_owned());
                if reply.delay_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(reply.delay_ms));
                }
                let reason = if reply.status == 200 { "OK" } else { "Error" };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    reply.status,
                    reason,
                    reply.body.len()
                );
                let _ = stream.write_all(&reply.body);
            }
        });
        (address, requests, server)
    }

    #[cfg(feature = "embeddings-ollama")]
    fn ollama_config(address: std::net::SocketAddr) -> EmbeddingConfig {
        use crate::config::EmbeddingProviderConfig;
        EmbeddingConfig {
            enabled: true,
            provider: EmbeddingProviderConfig::Ollama,
            endpoint: format!("http://{address}"),
            model: "fixture".into(),
            dimensions: 8,
            ..EmbeddingConfig::default()
        }
    }
    #[test]
    fn space_identity_tracks_contract() {
        let model = ResolvedEmbeddingModel {
            provider: EmbeddingProviderKind::Ollama,
            configured_name: "m".into(),
            canonical_name: "m:latest".into(),
            model_digest: "abc".into(),
            dimensions: None,
        };
        assert_eq!(
            EmbeddingSpace::new(&model, 3),
            EmbeddingSpace::new(&model, 3)
        );
        assert_ne!(
            EmbeddingSpace::new(&model, 3).id,
            EmbeddingSpace::new(&model, 4).id
        );
        let mut changed = model.clone();
        changed.model_digest = "def".into();
        assert_ne!(
            EmbeddingSpace::new(&model, 3).id,
            EmbeddingSpace::new(&changed, 3).id
        );
    }
    #[test]
    fn vectors_round_trip_and_reject_corruption() {
        let vector = normalize_vector(&[3.0, 4.0]).unwrap();
        let bytes = encode_vector(&vector, 2).unwrap();
        assert_eq!(decode_vector(&bytes, 2).unwrap().values, vector.values);
        assert!(decode_vector(&bytes[..4], 2).is_err());
        let raw = [3.0_f32, 4.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        assert!(matches!(
            decode_vector(&raw, 2),
            Err(EmbeddingError::CacheCorrupt(_))
        ));
        assert!(normalize_vector(&[0.0, 0.0]).is_err());
        assert!(normalize_vector(&[f32::NAN]).is_err());
    }
    #[test]
    fn disabled_provider_is_stable() {
        assert_eq!(
            DisabledEmbeddingProvider
                .resolve_model()
                .unwrap_err()
                .code(),
            "EMBEDDING_UNAVAILABLE"
        );
    }

    #[cfg(feature = "embeddings-ollama")]
    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn ollama_uses_current_bounded_endpoints() {
        use crate::config::{EmbeddingConfig, EmbeddingProviderConfig};
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::{Arc, Mutex};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured = Arc::clone(&requests);
        let server = std::thread::spawn(move || {
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let read = stream.read(&mut buffer).unwrap();
                    if read == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..read]);
                    let Some(headers_end) = bytes
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .map(|p| p + 4)
                    else {
                        continue;
                    };
                    let header = String::from_utf8_lossy(&bytes[..headers_end]);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= headers_end + length {
                        break;
                    }
                }
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&bytes).into_owned());
                let body = if index == 0 {
                    r#"{"models":[{"name":"fixture:latest","model":"fixture:latest","digest":"sha256:abc"}]}"#
                } else {
                    r#"{"model":"fixture:latest","embeddings":[[3.0,4.0,0.0,0.0,0.0,0.0,0.0,0.0]]}"#
                };
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let mut config = EmbeddingConfig::default();
        config.enabled = true;
        config.provider = EmbeddingProviderConfig::Ollama;
        config.endpoint = format!("http://{address}");
        config.model = "fixture".into();
        config.dimensions = 8;
        let provider = crate::embedding::ollama::OllamaProvider::new(&config).unwrap();
        let model = provider.resolve_model().unwrap();
        assert_eq!(model.canonical_name, "fixture:latest");
        assert_eq!(model.model_digest, "sha256:abc");
        let batch = provider
            .embed(
                &[EmbeddingInput {
                    text_hash: ContentHash("blake3:test".into()),
                    text: "private fixture".into(),
                }],
                &model,
            )
            .unwrap();
        assert_eq!(batch.vectors.len(), 1);
        server.join().unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("GET /api/tags "));
        assert!(requests[1].starts_with("POST /api/embed "));
        assert!(requests[1].contains("\"truncate\":false"));
        assert!(requests[1].contains("\"dimensions\":8"));
        assert!(
            requests
                .iter()
                .all(|request| !request.contains("/api/pull")
                    && !request.contains("/api/embeddings"))
        );
    }

    #[cfg(feature = "embeddings-ollama")]
    #[test]
    fn ollama_transport_rejects_http_malformed_oversized_timeout_and_missing_model() {
        let cases = vec![
            MockReply {
                status: 503,
                body: b"server detail must not escape".to_vec(),
                delay_ms: 0,
            },
            MockReply {
                status: 200,
                body: b"not json".to_vec(),
                delay_ms: 0,
            },
            MockReply {
                status: 200,
                body: vec![b' '; (32 * 1024 * 1024) + 1],
                delay_ms: 0,
            },
            MockReply {
                status: 200,
                body: br#"{"models":[]}"#.to_vec(),
                delay_ms: 0,
            },
        ];
        for reply in cases {
            let (address, _, server) = mock_server(vec![reply]);
            let provider =
                crate::embedding::ollama::OllamaProvider::new(&ollama_config(address)).unwrap();
            assert!(provider.resolve_model().is_err());
            server.join().unwrap();
        }
        let (address, _, server) = mock_server(vec![MockReply {
            status: 200,
            body: br#"{"models":[]}"#.to_vec(),
            delay_ms: 1200,
        }]);
        let mut config = ollama_config(address);
        config.request_timeout_seconds = 1;
        let provider = crate::embedding::ollama::OllamaProvider::new(&config).unwrap();
        assert!(provider.resolve_model().is_err());
        server.join().unwrap();
    }

    #[cfg(feature = "embeddings-ollama")]
    #[test]
    fn ollama_transport_rejects_invalid_embedding_responses() {
        let tags=br#"{"models":[{"name":"fixture:latest","model":"fixture:latest","digest":"sha256:abc"}]}"#;
        let invalid = [
            br#"{"model":"fixture:latest","embeddings":[]}"#.as_slice(),
            br#"{"model":"fixture:latest","embeddings":[[1,0,0,0,0,0,0,0],[1]]}"#.as_slice(),
            br#"{"model":"fixture:latest","embeddings":[[0,0,0,0,0,0,0,0]]}"#.as_slice(),
            br#"{"model":"other:latest","embeddings":[[1,0,0,0,0,0,0,0]]}"#.as_slice(),
            b"malformed".as_slice(),
        ];
        for body in invalid {
            let (address, requests, server) = mock_server(vec![
                MockReply {
                    status: 200,
                    body: tags.to_vec(),
                    delay_ms: 0,
                },
                MockReply {
                    status: 200,
                    body: body.to_vec(),
                    delay_ms: 0,
                },
            ]);
            let provider =
                crate::embedding::ollama::OllamaProvider::new(&ollama_config(address)).unwrap();
            let model = provider.resolve_model().unwrap();
            let inputs = [EmbeddingInput {
                text_hash: ContentHash("blake3:x".into()),
                text: "fixture".into(),
            }];
            assert!(provider.embed(&inputs, &model).is_err());
            server.join().unwrap();
            assert!(
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .all(|request| !request.contains("/api/pull")
                        && !request.contains("/api/embeddings"))
            );
        }
    }
}
