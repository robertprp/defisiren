use std::sync::atomic::{AtomicU64, Ordering};
#[derive(Default)]
pub struct Metrics {
    pub observations: AtomicU64,
    pub findings: AtomicU64,
    pub candidates: AtomicU64,
    pub traced: AtomicU64,
    pub untraced: AtomicU64,
    pub rpc_errors: AtomicU64,
    pub durability_errors: AtomicU64,
    pub source_gaps: AtomicU64,
    pub simulation_errors: AtomicU64,
    pub unsupported_inputs: AtomicU64,
}

impl Metrics {
    pub fn render(&self) -> String {
        let entries = [
            ("observations_total", &self.observations),
            ("findings_total", &self.findings),
            ("candidates_total", &self.candidates),
            ("traced_total", &self.traced),
            ("untraced_total", &self.untraced),
            ("rpc_errors_total", &self.rpc_errors),
            ("durability_errors_total", &self.durability_errors),
            ("source_gaps_total", &self.source_gaps),
            ("simulation_errors_total", &self.simulation_errors),
            ("unsupported_inputs_total", &self.unsupported_inputs),
        ];
        let mut out = String::new();
        for (name, value) in entries {
            out.push_str(&format!(
                "# TYPE chain_siren_{name} counter\nchain_siren_{name} {}\n",
                value.load(Ordering::Relaxed)
            ));
        }
        out
    }
}

pub fn init() {
    use std::io::IsTerminal;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,sqlx=warn,reqwest=warn,tungstenite=warn".into());
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr);
    match std::env::var("SIREN_LOG_FORMAT").as_deref() {
        Ok("json") => {
            let _ = builder.json().try_init();
        }
        Ok("pretty") => {
            let _ = builder
                .with_ansi(std::io::stderr().is_terminal())
                .pretty()
                .try_init();
        }
        _ => {
            let _ = builder
                .with_ansi(std::io::stderr().is_terminal())
                .compact()
                .try_init();
        }
    }
}
