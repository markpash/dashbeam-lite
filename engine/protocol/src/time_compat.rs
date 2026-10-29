pub use std::time::{Duration, Instant};

pub async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}

pub async fn timeout<F, T>(duration: Duration, future: F) -> Result<T, tokio::time::error::Elapsed>
where
    F: std::future::Future<Output = T>,
{
    tokio::time::timeout(duration, future).await
}
