use std::sync::Arc;
use tokio::time::{Duration, interval};

use crate::AppState;

/// Runs every 5 minutes, tears down expired deployments.
pub async fn run_cleanup_loop(state: Arc<AppState>) {
    let mut ticker = interval(Duration::from_secs(300));

    loop {
        ticker.tick().await;

        let now = chrono::Utc::now().to_rfc3339();
        let expired = match state.db.get_expired_deployments(&now) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("cleanup: failed to query expired deployments: {e}");
                continue;
            }
        };

        for deployment in expired {
            tracing::info!("cleanup: tearing down expired deployment '{}'", deployment.name);

            if let Some(ref container_id) = deployment.container_id {
                if let Err(e) = state.docker.stop_container(container_id).await {
                    tracing::error!("cleanup: failed to stop container {}: {e}", container_id);
                }
            }

            if let Err(e) = state.proxy.remove_route(&deployment.name).await {
                tracing::error!("cleanup: failed to remove proxy route for '{}': {e}", deployment.name);
            }

            if let Err(e) = state.db.delete_deployment(&deployment.name) {
                tracing::error!("cleanup: failed to delete deployment record '{}': {e}", deployment.name);
            }
        }
    }
}
