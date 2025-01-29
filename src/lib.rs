use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

pub mod clients;
pub mod events;
pub mod logs;
pub mod metrics;

pub fn set_up_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::WARN.into())
                .from_env_lossy(),
        )
        .with_ansi(false)
        .init();
}
