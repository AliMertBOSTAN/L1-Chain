//! Graceful shutdown signal handling (Ctrl-C, SIGTERM).

/// Returns a future that resolves when a shutdown signal is received.
///
/// On Unix: listens for SIGTERM and Ctrl-C.
/// On Windows: listens for Ctrl-C only.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        // Installing the Ctrl-C handler only fails when the runtime is
        // misconfigured — log and exit cleanly instead of panicking.
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(?err, "failed to install Ctrl-C handler");
        }
    };

    #[cfg(unix)]
    let sigterm = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(err) => {
                tracing::error!(?err, "failed to install SIGTERM handler");
            }
        }
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
        // We don't await the future — explicitly drop it so clippy doesn't
        // complain about `let _ = <future>`.
        drop(shutdown_signal());
    }
}
