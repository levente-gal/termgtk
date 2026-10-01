fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    if let Err(err) = herdr_native_poc::app::run() {
        eprintln!("herdr-native-poc: {err:#}");
        std::process::exit(1);
    }
}
