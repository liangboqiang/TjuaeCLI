use std::env;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;

use tjuae_config::config::{Config, ProviderType};
use tjuae_types::llm::{LlmEvent, LlmRequest};

use crate::anthropic;
use crate::bedrock;
use crate::error::ProviderError;
use crate::openai;
use crate::vertex;

/// Unified interface for LLM API providers
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream(&self, request: &LlmRequest) -> Result<mpsc::Receiver<LlmEvent>, ProviderError>;
}

/// Create a provider from resolved config
pub fn create_provider(config: &Config) -> Arc<dyn LlmProvider> {
    create_provider_with_client(config, reqwest::Client::new())
}

/// Create a provider with a caller-owned HTTP client.
///
/// Embedding hosts use this entry point to apply a single transport policy
/// (for example proxy routing) across every built-in provider.
pub fn create_provider_with_client(config: &Config, client: reqwest::Client) -> Arc<dyn LlmProvider> {
    let compat = config.compat.clone();

    match config.provider {
        ProviderType::Anthropic => Arc::new(
            anthropic::AnthropicProvider::new_with_client(client, &config.api_key, &config.base_url, compat)
                .with_cache(config.prompt_caching),
        ),
        ProviderType::OpenAI => Arc::new(openai::OpenAIProvider::new_with_client(
            client,
            &config.api_key,
            &config.base_url,
            compat,
        )),
        ProviderType::Bedrock => {
            let bc = config.bedrock.clone().unwrap_or_default();
            let region = bc
                .region
                .clone()
                .or_else(|| env::var("AWS_REGION").ok())
                .or_else(|| env::var("AWS_DEFAULT_REGION").ok())
                .unwrap_or_else(|| "us-east-1".to_string());
            let credentials = bedrock::credentials_from_config(&bc);
            Arc::new(bedrock::BedrockProvider::new_with_client(
                client,
                &region,
                credentials,
                config.prompt_caching,
                compat,
            ))
        }
        ProviderType::Vertex => {
            let vc = config.vertex.clone().unwrap_or_default();
            let project_id = vc.project_id.clone().unwrap_or_default();
            let region = vc.region.clone().unwrap_or_else(|| "us-central1".to_string());
            let auth = vertex::auth_from_config(&vc);
            Arc::new(vertex::VertexProvider::new_with_client(
                client,
                &project_id,
                &region,
                auth,
                config.prompt_caching,
                compat,
            ))
        }
    }
}

#[cfg(test)]
#[path = "provider_test.rs"]
mod provider_test;
