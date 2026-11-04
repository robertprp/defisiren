fn main() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let cfg = rt
        .block_on(siren_service::config::Config::load(std::path::Path::new(
            "config/example.toml",
        )))
        .expect("config");
    siren_service::benchmark(100_000, &cfg).expect("benchmark");
}
