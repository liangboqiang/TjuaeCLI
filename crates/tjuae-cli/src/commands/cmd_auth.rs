use tjuae_config::auth::{AuthConfig, OAuthManager};
use tjuae_config::config::load_global_auth_config;

use crate::cli::AuthAction;

pub(crate) async fn run(action: AuthAction) -> anyhow::Result<()> {
    match action {
        AuthAction::Login => {
            let oauth = OAuthManager::new(load_global_auth_config()?);
            oauth.login().await?;
            eprintln!("Login successful! You can now use tjuae-cli without --api-key.");
            Ok(())
        }
        AuthAction::Logout => OAuthManager::new(AuthConfig::default()).logout(),
    }
}
