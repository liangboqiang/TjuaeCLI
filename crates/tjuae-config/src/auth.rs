// OAuth 2.0 Device Authorization Flow for Claude.ai subscriber accounts.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Stored OAuth credentials
#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthCredentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub token_type: String,
}

/// OAuth device code response
#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// OAuth token response
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    token_type: String,
}

/// OAuth token error response (during polling)
#[derive(Debug, Deserialize)]
struct TokenErrorResponse {
    error: String,
}

/// Config for OAuth endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_auth_url")]
    pub auth_url: String,
    #[serde(default = "default_token_url")]
    pub token_url: String,
    #[serde(default)]
    pub client_id: String,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            auth_url: default_auth_url(),
            token_url: default_token_url(),
            client_id: String::new(),
        }
    }
}

fn default_auth_url() -> String {
    "https://claude.ai/oauth".to_string()
}

fn default_token_url() -> String {
    "https://claude.ai/oauth/token".to_string()
}

pub struct OAuthManager {
    client: reqwest::Client,
    config: AuthConfig,
    credentials_path: PathBuf,
}

impl OAuthManager {
    pub fn new(config: AuthConfig) -> Self {
        let credentials_path = crate::config::app_config_dir()
            .unwrap_or_else(|| PathBuf::from("tjuae"))
            .join("auth.json");

        Self {
            client: reqwest::Client::new(),
            config,
            credentials_path,
        }
    }

    /// Full device authorization flow
    pub async fn login(&self) -> anyhow::Result<OAuthCredentials> {
        let client_id = self.config.require_client_id()?;

        // Step 1: Request device code
        let device_code_url = format!("{}/device/code", self.config.auth_url);
        let resp = self
            .client
            .post(&device_code_url)
            .form(&[("client_id", client_id), ("scope", "user:inference")])
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("请求设备代码失败：{}", body);
        }

        let device_resp: DeviceCodeResponse = resp.json().await?;

        // Step 2: Display instructions
        eprintln!();
        eprintln!("  要完成身份认证，请访问：");
        eprintln!("  {}", device_resp.verification_uri);
        eprintln!();
        eprintln!("  输入代码：{}", device_resp.user_code);
        eprintln!();
        eprintln!("  正在等待授权……");

        // Step 3: Poll for token
        let interval = std::time::Duration::from_secs(device_resp.interval.max(5));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(device_resp.expires_in);

        loop {
            if std::time::Instant::now() > deadline {
                anyhow::bail!("设备授权超时，请重试。");
            }

            tokio::time::sleep(interval).await;

            let token_resp = self
                .client
                .post(&self.config.token_url)
                .form(&[
                    ("client_id", client_id),
                    ("device_code", device_resp.device_code.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .send()
                .await?;

            let status = token_resp.status();
            let body = token_resp.text().await.unwrap_or_default();

            if status.is_success() {
                let token: TokenResponse = serde_json::from_str(&body)?;
                let credentials = OAuthCredentials {
                    access_token: token.access_token,
                    refresh_token: token.refresh_token,
                    expires_at: Utc::now() + chrono::Duration::seconds(token.expires_in as i64),
                    token_type: token.token_type,
                };
                self.save_credentials(&credentials)?;
                return Ok(credentials);
            }

            // Check if we should keep polling
            if let Ok(err_resp) = serde_json::from_str::<TokenErrorResponse>(&body) {
                match err_resp.error.as_str() {
                    "authorization_pending" => continue,
                    "slow_down" => {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                    "expired_token" => {
                        anyhow::bail!("设备代码已过期，请重试。");
                    }
                    "access_denied" => {
                        anyhow::bail!("用户拒绝了授权。");
                    }
                    other => {
                        anyhow::bail!("OAuth 错误：{}", other);
                    }
                }
            }

            anyhow::bail!("收到意外的 OAuth 响应：{}", body);
        }
    }

    /// Get a valid access token (refresh if expired)
    pub async fn get_token(&self) -> anyhow::Result<String> {
        let creds = self.load_credentials()?;

        if creds.expires_at > Utc::now() + chrono::Duration::minutes(1) {
            return Ok(creds.access_token);
        }

        // Try refresh
        if let Some(refresh_token) = &creds.refresh_token {
            let new_creds = self.refresh(refresh_token).await?;
            self.save_credentials(&new_creds)?;
            return Ok(new_creds.access_token);
        }

        anyhow::bail!("token 已过期且没有可用的刷新 token。请运行 'tjuae-cli auth login'")
    }

    /// Refresh the access token
    async fn refresh(&self, refresh_token: &str) -> anyhow::Result<OAuthCredentials> {
        let client_id = self.config.require_client_id()?;
        let resp = self
            .client
            .post(&self.config.token_url)
            .form(&[
                ("client_id", client_id),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("刷新 token 失败：{}", body);
        }

        let token: TokenResponse = resp.json().await?;
        Ok(OAuthCredentials {
            access_token: token.access_token,
            refresh_token: token.refresh_token.or(Some(refresh_token.to_string())),
            expires_at: Utc::now() + chrono::Duration::seconds(token.expires_in as i64),
            token_type: token.token_type,
        })
    }

    /// Logout: delete saved credentials
    pub fn logout(&self) -> anyhow::Result<()> {
        if self.credentials_path.exists() {
            std::fs::remove_file(&self.credentials_path)?;
            eprintln!("凭据已删除：{}", self.credentials_path.display());
        } else {
            eprintln!("未找到已保存的凭据。");
        }
        Ok(())
    }

    /// Check if credentials exist
    pub fn has_credentials(&self) -> bool {
        self.credentials_path.exists()
    }

    fn save_credentials(&self, creds: &OAuthCredentials) -> anyhow::Result<()> {
        if let Some(parent) = self.credentials_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(creds)?;
        std::fs::write(&self.credentials_path, json)?;
        Ok(())
    }

    fn load_credentials(&self) -> anyhow::Result<OAuthCredentials> {
        let json = std::fs::read_to_string(&self.credentials_path)
            .map_err(|_| anyhow::anyhow!("未找到已保存的凭据。请运行 'tjuae-cli auth login'"))?;
        let creds: OAuthCredentials = serde_json::from_str(&json)?;
        Ok(creds)
    }
}

impl AuthConfig {
    /// Return the explicitly configured OAuth client identifier.
    pub(crate) fn require_client_id(&self) -> anyhow::Result<&str> {
        let client_id = self.client_id.trim();
        if client_id.is_empty() {
            anyhow::bail!(
                "尚未配置 OAuth client_id。请在 {} 的 `[auth]` 节中设置 `client_id`。",
                crate::config::global_config_path().display()
            );
        }
        Ok(client_id)
    }
}

#[cfg(test)]
#[path = "auth_test.rs"]
mod auth_test;
