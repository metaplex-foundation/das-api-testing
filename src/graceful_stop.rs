use tokio::signal;
use tokio::task::{JoinError, JoinSet};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub async fn listen_shutdown(cancel_token: CancellationToken) {
    tokio::spawn(async move {
        match signal::ctrl_c().await {
            Ok(()) => {
                cancel_token.cancel();
            }
            Err(err) => {
                error!("Unable to listen for shutdown signal: {}", err);
            }
        }
    });
}

pub async fn graceful_stop(tasks: &mut JoinSet<Result<(), JoinError>>) {
    while let Some(task) = tasks.join_next().await {
        match task {
            Ok(_) => {}
            Err(err) if err.is_panic() => {
                let err = err.into_panic();
                error!("Task panic: {:?}", err);
            }
            Err(err) => {
                let err = err.to_string();
                error!("Task error: {}", err);
            }
        }
    }
}
