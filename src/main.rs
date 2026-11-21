//! Desktop entry point for the iced application.

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn main() -> iced::Result {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(plsqlide::core::postgres_spike::notice_capture_layer())
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .with(filter)
        .init();

    plsqlide::ui::run()
}
