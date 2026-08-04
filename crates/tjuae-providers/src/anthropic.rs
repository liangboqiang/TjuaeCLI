use async_trait::async_trait;
#[cfg(test)]
use serde_json::Value;
use tokio::sync::mpsc;

use tjuae_types::llm::{LlmEvent, LlmRequest};

use crate::composed::ComposedProvider;
use crate::transport::{AnthropicTransport, ProviderTransport};
use crate::{LlmProvider, ProviderError};
use tjuae_config::compat::ProviderCompat;

pub struct AnthropicProvider {
    inner: ComposedProvider,
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    compat: ProviderCompat,
    cache_enabled: bool,
}

impl AnthropicProvider {
    pub fn new(api_key: &str, base_url: &str, compat: ProviderCompat) -> Self {
        Self::new_with_client(reqwest::Client::new(), api_key, base_url, compat)
    }

    pub fn new_with_client(client: reqwest::Client, api_key: &str, base_url: &str, compat: ProviderCompat) -> Self {
        let cache_enabled = true;
        let inner = Self::build_inner(client.clone(), api_key, base_url, cache_enabled, &compat);

        Self {
            inner,
            client,
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            compat,
            cache_enabled,
        }
    }

    pub fn with_cache(mut self, enabled: bool) -> Self {
        self.cache_enabled = enabled;
        self.inner = Self::build_inner(
            self.client.clone(),
            &self.api_key,
            &self.base_url,
            self.cache_enabled,
            &self.compat,
        );
        self
    }

    fn build_inner(
        client: reqwest::Client,
        api_key: &str,
        base_url: &str,
        cache_enabled: bool,
        compat: &ProviderCompat,
    ) -> ComposedProvider {
        let transport = ProviderTransport::Anthropic(AnthropicTransport::new_with_client(
            client,
            api_key,
            base_url,
            cache_enabled,
        ));
        ComposedProvider::new(transport, compat.clone())
    }

    #[cfg(test)]
    fn build_request_body(&self, request: &LlmRequest) -> Result<Value, ProviderError> {
        self.inner.build_request_body(request)
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn stream(&self, request: &LlmRequest) -> Result<mpsc::Receiver<LlmEvent>, ProviderError> {
        self.inner.stream(request).await
    }
}

#[cfg(test)]
#[path = "anthropic_test.rs"]
mod anthropic_test;
