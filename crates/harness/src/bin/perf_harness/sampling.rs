use std::future::Future;
use std::time::Instant;

use anyhow::Result;
use serde::Serialize;

pub(super) const MAX_PYTHON_RATIO: f64 = 1.15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Server {
    Rust,
    Python,
}

#[derive(Debug, Serialize)]
pub(super) struct Pair {
    pub first: Server,
    pub rust_started_ms: f64,
    pub python_started_ms: f64,
    pub rust_ms: f64,
    pub python_ms: f64,
}

pub(super) async fn paired_samples<F, Measurement>(count: usize, mut sample: F) -> Result<Vec<Pair>>
where
    F: FnMut(Server) -> Measurement,
    Measurement: Future<Output = Result<f64>>,
{
    anyhow::ensure!(count > 0 && count.is_multiple_of(2), "pair count must be positive and even");
    let origin = Instant::now();
    let mut pairs = Vec::with_capacity(count);
    for index in 0..count {
        let (first, second) = if index.is_multiple_of(2) {
            (Server::Rust, Server::Python)
        } else {
            (Server::Python, Server::Rust)
        };
        let first_started = origin.elapsed().as_secs_f64() * 1_000.0;
        let first_ms = sample(first).await?;
        valid(first_ms)?;
        let second_started = origin.elapsed().as_secs_f64() * 1_000.0;
        let second_ms = sample(second).await?;
        valid(second_ms)?;
        let pair = match first {
            Server::Rust => Pair {
                first,
                rust_started_ms: first_started,
                python_started_ms: second_started,
                rust_ms: first_ms,
                python_ms: second_ms,
            },
            Server::Python => Pair {
                first,
                rust_started_ms: second_started,
                python_started_ms: first_started,
                rust_ms: second_ms,
                python_ms: first_ms,
            },
        };
        pairs.push(pair);
    }
    Ok(pairs)
}

pub(super) fn p95(samples: &[f64]) -> Result<f64> {
    anyhow::ensure!(!samples.is_empty(), "performance sample is empty");
    for value in samples {
        valid(*value)?;
    }
    let mut ordered = samples.to_vec();
    ordered.sort_by(f64::total_cmp);
    let index = (ordered.len() * 95).div_ceil(100).saturating_sub(1);
    ordered.get(index).copied().ok_or_else(|| anyhow::anyhow!("p95 index is invalid"))
}

pub(super) fn enforce_ratio(ratio: f64) -> Result<()> {
    anyhow::ensure!(
        ratio.is_finite() && ratio > 0.0 && ratio <= MAX_PYTHON_RATIO,
        "fake-EAS p95 is more than 15% slower than the Python baseline"
    );
    Ok(())
}

fn valid(value: f64) -> Result<()> {
    anyhow::ensure!(value.is_finite() && value > 0.0, "latency sample is not finite and positive");
    Ok(())
}
