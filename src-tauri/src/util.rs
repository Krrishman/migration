//! Small shared helpers: size formatting, throughput/ETA estimation,
//! cancellation tokens and atomic JSON writes.

use crate::error::{AppError, AppResult, IoContext};
use serde::Serialize;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Human-readable binary size ("1.5 GB" uses 1024 multiples, Windows style).
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["bytes", "KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return if bytes == 1 { "1 byte".into() } else { format!("{bytes} bytes") };
    }
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[unit])
    } else if v >= 10.0 {
        format!("{v:.1} {}", UNITS[unit])
    } else {
        format!("{v:.2} {}", UNITS[unit])
    }
}

pub fn format_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h} h {m:02} min")
    } else if m > 0 {
        format!("{m} min {s:02} s")
    } else {
        format!("{s} s")
    }
}

/// Exponentially smoothed throughput with an honest ETA: no estimate until
/// enough data has been observed, or when the total is unknown.
#[derive(Debug, Clone)]
pub struct RateEstimator {
    started: Instant,
    last_sample: Instant,
    last_bytes: u64,
    rate: Option<f64>,
    alpha: f64,
    min_elapsed: Duration,
    min_bytes: u64,
}

impl RateEstimator {
    pub fn new() -> Self {
        Self::starting_at(Instant::now())
    }

    pub fn starting_at(now: Instant) -> Self {
        Self {
            started: now,
            last_sample: now,
            last_bytes: 0,
            rate: None,
            alpha: 0.3,
            min_elapsed: Duration::from_secs(3),
            min_bytes: 1024 * 1024,
        }
    }

    pub fn sample(&mut self, bytes_done: u64, now: Instant) {
        let dt = now.duration_since(self.last_sample).as_secs_f64();
        if dt < 0.25 {
            return;
        }
        let inst = bytes_done.saturating_sub(self.last_bytes) as f64 / dt;
        self.rate = Some(match self.rate {
            None => inst,
            Some(r) => self.alpha * inst + (1.0 - self.alpha) * r,
        });
        self.last_sample = now;
        self.last_bytes = bytes_done;
    }

    pub fn rate(&self) -> Option<f64> {
        self.rate
    }

    pub fn elapsed(&self, now: Instant) -> Duration {
        now.duration_since(self.started)
    }

    /// Seconds remaining, or `None` when it cannot be estimated meaningfully.
    pub fn eta_seconds(&self, bytes_done: u64, bytes_total: Option<u64>, now: Instant) -> Option<u64> {
        let total = bytes_total?;
        if bytes_done >= total {
            return Some(0);
        }
        if self.elapsed(now) < self.min_elapsed || bytes_done < self.min_bytes {
            return None;
        }
        let rate = self.rate?;
        if rate < 1.0 {
            return None;
        }
        let eta = ((total - bytes_done) as f64 / rate).ceil();
        // Beyond a week the estimate is meaningless.
        if eta > 7.0 * 24.0 * 3600.0 { None } else { Some(eta as u64) }
    }
}

impl Default for RateEstimator {
    fn default() -> Self {
        Self::new()
    }
}

/// Cooperative cancellation shared between UI commands and worker threads.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_canceled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn check(&self) -> AppResult<()> {
        if self.is_canceled() { Err(AppError::Canceled) } else { Ok(()) }
    }
}

/// Write JSON to `<path>.tmp`, fsync, then rename over `path`. On Windows
/// `std::fs::rename` uses MoveFileEx with REPLACE_EXISTING, which is atomic
/// on the same NTFS volume.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_bytes_atomic(path, &bytes)
}

pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default()
    ));
    {
        let mut f = std::fs::File::create(&tmp).at(&tmp)?;
        f.write_all(bytes).at(&tmp)?;
        f.sync_all().at(&tmp)?;
    }
    std::fs::rename(&tmp, path).at(path)?;
    Ok(())
}

pub fn now_utc() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes() {
        assert_eq!(format_bytes(0), "0 bytes");
        assert_eq!(format_bytes(1), "1 byte");
        assert_eq!(format_bytes(1023), "1023 bytes");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(10 * 1024 * 1024), "10.0 MB");
        assert_eq!(format_bytes(250 * 1024 * 1024 * 1024), "250 GB");
        assert!(format_bytes(u64::MAX).ends_with(" PB"));
    }

    #[test]
    fn formats_durations() {
        assert_eq!(format_duration(5), "5 s");
        assert_eq!(format_duration(65), "1 min 05 s");
        assert_eq!(format_duration(3700), "1 h 01 min");
    }

    #[test]
    fn eta_is_withheld_until_meaningful() {
        let t0 = Instant::now();
        let mut r = RateEstimator::starting_at(t0);
        // Too early.
        r.sample(512 * 1024, t0 + Duration::from_millis(500));
        assert_eq!(r.eta_seconds(512 * 1024, Some(100 * 1024 * 1024), t0 + Duration::from_millis(500)), None);
        // Unknown total.
        r.sample(10 * 1024 * 1024, t0 + Duration::from_secs(4));
        assert_eq!(r.eta_seconds(10 * 1024 * 1024, None, t0 + Duration::from_secs(4)), None);
    }

    #[test]
    fn eta_converges_on_steady_rate() {
        let t0 = Instant::now();
        let mut r = RateEstimator::starting_at(t0);
        let mb = 1024 * 1024u64;
        for s in 1..=10u64 {
            r.sample(s * 10 * mb, t0 + Duration::from_secs(s));
        }
        let eta = r.eta_seconds(100 * mb, Some(200 * mb), t0 + Duration::from_secs(10)).unwrap();
        assert!((9..=11).contains(&eta), "eta {eta}");
        assert_eq!(r.eta_seconds(200 * mb, Some(200 * mb), t0 + Duration::from_secs(10)), Some(0));
    }

    #[test]
    fn atomic_json_write_replaces_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("m.json");
        write_json_atomic(&p, &serde_json::json!({"a": 1})).unwrap();
        write_json_atomic(&p, &serde_json::json!({"a": 2})).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(v["a"], 2);
        assert!(!dir.path().join("m.json.tmp").exists());
    }

    #[test]
    fn cancel_token() {
        let t = CancelToken::new();
        assert!(t.check().is_ok());
        t.clone().cancel();
        assert!(matches!(t.check(), Err(AppError::Canceled)));
    }
}
