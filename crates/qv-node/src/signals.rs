//! Graceful shutdown signal handling (Ctrl-C, SIGTERM).

/// Returns a future that resolves when a shutdown signal is received.
///
/// On Unix: listens for SIGTERM and Ctrl-C.
/// On Windows: listens for Ctrl-C only.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let sigterm = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("received Ctrl-C signal");
        }
        _ = sigterm => {
            tracing::info!("received SIGTERM signal");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shutdown_signal_compiles() {
        // Just ensure the function compiles and the types are correct.
        let signal_fut = shutdown_signal();
        // Don't await it in the test; we just want to verify it's a valid future.
        let _ = signal_fut;
    }
}
