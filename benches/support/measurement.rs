use crate::Result;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Statistics {
    pub min: f64,
    pub median: f64,
    pub p95: f64,
    pub max: f64,
    pub mean: f64,
    pub stddev: f64,
}

impl Statistics {
    /// Nearest-rank p95; for short runs this intentionally equals the maximum.
    pub fn from_samples(samples: &[f64]) -> Self {
        assert!(!samples.is_empty());
        let mut sorted = samples.to_vec();
        sorted.sort_by(f64::total_cmp);
        let count = sorted.len();
        let mean = sorted.iter().sum::<f64>() / count as f64;
        let variance = sorted
            .iter()
            .map(|sample| (sample - mean).powi(2))
            .sum::<f64>()
            / count as f64;
        Self {
            min: sorted[0],
            median: if count.is_multiple_of(2) {
                (sorted[count / 2 - 1] + sorted[count / 2]) / 2.0
            } else {
                sorted[count / 2]
            },
            p95: sorted[(count * 95).div_ceil(100) - 1],
            max: sorted[count - 1],
            mean,
            stddev: variance.sqrt(),
        }
    }
}

pub struct Measurement {
    pub samples_ms: Vec<f64>,
    pub batch: usize,
}

/// Calibrate outside the measured samples, then amortize clock overhead in batches.
/// Output construction/destruction belongs to each operation; setup belongs outside.
pub fn measure(
    samples: usize,
    target: Duration,
    mut operation: impl FnMut() -> Result<()>,
) -> Result<Measurement> {
    let start = Instant::now();
    operation()?;
    let elapsed = start.elapsed().as_nanos().max(1);
    let batch = target.as_nanos().div_ceil(elapsed).clamp(1, 1_000_000) as usize;
    let mut samples_ms = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        for _ in 0..batch {
            operation()?;
        }
        samples_ms.push(start.elapsed().as_secs_f64() * 1000.0 / batch as f64);
    }
    Ok(Measurement { samples_ms, batch })
}
