#[path = "../src/bin/perf_harness/sampling.rs"]
mod sampling;

use std::future::ready;

use sampling::{Pair, Server, enforce_ratio, p95, paired_samples};

fn ratio(pairs: &[Pair]) -> anyhow::Result<f64> {
    let rust = pairs.iter().map(|pair| pair.rust_ms).collect::<Vec<_>>();
    let python = pairs.iter().map(|pair| pair.python_ms).collect::<Vec<_>>();
    Ok(p95(&rust)? / p95(&python)?)
}

#[tokio::test]
async fn common_load_drift_biases_block_phases_but_not_adjacent_pairs() -> anyhow::Result<()> {
    // Identical servers experience a common load change halfway through 400 calls.
    // Sequential phases attribute the entire high-load interval to Rust.
    let blocked_rust = vec![4.0; 200];
    let blocked_python = vec![2.0; 200];
    assert!(enforce_ratio(p95(&blocked_rust)? / p95(&blocked_python)?).is_err());
    let mut calls = 0;
    let paired = paired_samples(200, |_| {
        let latency = if calls < 200 { 4.0 } else { 2.0 };
        calls += 1;
        ready(Ok(latency))
    })
    .await?;
    assert_eq!(calls, 400);
    assert_eq!(ratio(&paired)?, 1.0);
    enforce_ratio(ratio(&paired)?)?;
    Ok(())
}

#[tokio::test]
async fn real_twenty_percent_slowdown_remains_a_failure_under_the_same_drift() -> anyhow::Result<()>
{
    let mut calls = 0;
    let paired = paired_samples(200, |server| {
        let common = if calls < 200 { 4.0 } else { 2.0 };
        calls += 1;
        ready(Ok(common * if server == Server::Rust { 1.2 } else { 1.0 }))
    })
    .await?;
    assert!((ratio(&paired)? - 1.2).abs() < 1e-12);
    assert!(enforce_ratio(ratio(&paired)?).is_err());
    Ok(())
}

#[tokio::test]
async fn order_is_balanced_and_outliers_are_retained() -> anyhow::Result<()> {
    let mut calls = Vec::new();
    let pairs = paired_samples(200, |server| {
        calls.push(server);
        ready(Ok(if calls.len() == 11 { 100.0 } else { 1.0 }))
    })
    .await?;
    assert_eq!(pairs.len(), 200);
    assert_eq!(pairs.iter().filter(|pair| pair.first == Server::Rust).count(), 100);
    assert_eq!(pairs.iter().filter(|pair| pair.first == Server::Python).count(), 100);
    assert!(calls.chunks_exact(4).all(|values| matches!(
        values,
        [Server::Rust, Server::Python, Server::Python, Server::Rust]
    )));
    assert_eq!(
        pairs
            .iter()
            .flat_map(|pair| [pair.rust_ms, pair.python_ms])
            .filter(|value| *value == 100.0)
            .count(),
        1
    );
    for pair in &pairs {
        assert!(pair.rust_started_ms >= 0.0 && pair.python_started_ms >= 0.0);
        assert!(match pair.first {
            Server::Rust => pair.rust_started_ms <= pair.python_started_ms,
            Server::Python => pair.python_started_ms <= pair.rust_started_ms,
        });
    }
    Ok(())
}

#[tokio::test]
async fn failed_sample_stops_the_run_without_retrying() {
    let mut calls = 0;
    let result = paired_samples(200, |_| {
        calls += 1;
        ready(if calls == 7 { Err(anyhow::anyhow!("scripted failure")) } else { Ok(1.0) })
    })
    .await;
    assert!(result.is_err());
    assert_eq!(calls, 7);
}

#[tokio::test]
async fn invalid_pair_count_does_not_start_sampling() {
    for count in [0, 1, 3] {
        let mut calls = 0;
        let result = paired_samples(count, |_| {
            calls += 1;
            ready(Ok(1.0))
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls, 0);
    }
}

#[test]
fn thresholds_use_raw_values_and_invalid_samples_are_not_discarded() -> anyhow::Result<()> {
    enforce_ratio(1.15)?;
    assert!(enforce_ratio(1.1504).is_err());
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(enforce_ratio(invalid).is_err());
        assert!(p95(&[1.0, invalid]).is_err());
    }
    assert!(p95(&[]).is_err());
    // Nearest-rank p95 remains the 190th observation for 200 values.
    assert_eq!(p95(&(1..=200).map(f64::from).collect::<Vec<_>>())?, 190.0);
    Ok(())
}
