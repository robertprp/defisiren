use crate::{config::Config, detection, events::Accepted};
use siren_lib::{AppResult, Error};
use std::{collections::HashSet, hint::black_box, time::Instant};
pub fn run(iterations: usize, cfg: &Config) -> AppResult<()> {
    if iterations == 0 || iterations > 10_000_000 {
        return Err(Error::Limit("benchmark iterations".into()));
    }
    let line = include_str!("../../fixtures/demo.jsonl")
        .lines()
        .next()
        .ok_or_else(|| Error::Invalid("benchmark fixture".into()))?;
    let mut dedup = HashSet::with_capacity(65536);
    let mut latencies = Vec::with_capacity(iterations.min(100_000));
    let mut count = 0;
    let start = Instant::now();
    for i in 0..iterations {
        let sample = Instant::now();
        let event: Accepted = serde_json::from_str(black_box(line))?;
        if let Accepted::Block(block) = event {
            for o in block.observations {
                black_box(dedup.insert((o.chain_id, o.hash, o.stage)));
                black_box(detection::candidate(&o, cfg));
                count += 1;
            }
        }
        if i % 100 == 0 {
            latencies.push(sample.elapsed().as_nanos() as u64);
        }
        if dedup.len() >= 65536 {
            dedup.clear();
        }
    }
    let elapsed = start.elapsed();
    latencies.sort_unstable();
    let p99 = latencies
        .get(latencies.len().saturating_sub(1) * 99 / 100)
        .copied();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"kind":"warm_decode_dedup_screen_only","envelopes":count,"seconds":elapsed.as_secs_f64(),"envelopes_per_second":count as f64/elapsed.as_secs_f64(),"sampled_p99_ns":p99,"fixture_bytes":line.len(),"remote_io":false,"durability":false,"unique_workload":false,"hardware":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"logical_cpus":std::thread::available_parallelism().map(|n|n.get()).ok()},"warning":"Repeated synthetic envelope microbenchmark. Not exploit-analysis throughput or sustained unique load."})
        )?
    );
    Ok(())
}
