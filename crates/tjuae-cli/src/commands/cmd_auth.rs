use tjuae_config::auth::{AuthConfig, OAuthManager};
use tjuae_config::config::load_global_auth_config;

use crate::cli::AuthAction;

pub(crate) async fn run(action: AuthAction) -> anyhow::Result<()> {
    match action {
        AuthAction::Login => {
            let oauth = OAuthManager::new(load_global_auth_config()?);
            oauth.login().await?;
            eprintln!("登录成功！现在可以在不指定 --api-key 的情况下使用 tjuae-cli。");
            Ok(())
        }
        AuthAction::Logout => OAuthManager::new(AuthConfig::default()).logout(),
    }
}
