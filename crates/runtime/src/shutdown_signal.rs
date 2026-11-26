//! [`termination_signal`]: the default shutdown trigger.

/// Resolves on the first of `SIGTERM` or `SIGINT`, logging which one fired.
///
/// # Errors
/// [`std::io::Error`] if a signal handler cannot be registered.
#[cfg(unix)]
pub async fn termination_signal() -> std::io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;

    let signal = tokio::select! {
        _ = terminate.recv() => "SIGTERM",
        _ = interrupt.recv() => "SIGINT",
    };
    tracing::info!(signal, "termination signal received");

    Ok(())
}

/// Resolves when the process is asked to terminate (Ctrl-C on non-Unix targets).
///
/// # Errors
/// [`std::io::Error`] if the Ctrl-C handler cannot be registered.
#[cfg(not(unix))]
pub async fn termination_signal() -> std::io::Result<()> {
    tokio::signal::ctrl_c().await?;
    tracing::info!(signal = "CTRL_C", "termination signal received");
    Ok(())
}
