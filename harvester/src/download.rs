//! Polite, resumable Jamendo MP3 downloader.
//!
//! The whole point is to NOT hammer Jamendo over an overnight run: a global
//! token-bucket caps requests/second, bounded concurrency caps simultaneous
//! connections, and failed requests back off exponentially. Already-downloaded
//! files are reused (resume), so a restart re-fetches nothing.

use anyhow::{anyhow, Result};
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use sha2::{Digest, Sha256};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Downloader {
    client: reqwest::Client,
    limiter: DefaultDirectRateLimiter,
    out_dir: PathBuf,
    max_retries: u32,
}

pub struct Fetched {
    pub path: String,
    pub bytes: i64,
    pub sha256: String,
}

impl Downloader {
    pub fn new(out_dir: impl Into<PathBuf>, rps: u32, timeout_s: u64) -> Result<Self> {
        let rps = NonZeroU32::new(rps.max(1)).unwrap();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_s))
            .user_agent("hackatune-harvester/0.1 (research; rate-limited)")
            .build()?;
        Ok(Self {
            client,
            limiter: RateLimiter::direct(Quota::per_second(rps)),
            out_dir: out_dir.into(),
            max_retries: 6,
        })
    }

    fn dest(&self, jamendo_id: &str) -> PathBuf {
        self.out_dir.join(format!("{jamendo_id}.mp3"))
    }

    fn url(jamendo_id: &str) -> String {
        format!("https://prod-1.storage.jamendo.com/download/track/{jamendo_id}/mp32/")
    }

    /// Fetch (or reuse) one track's MP3. Rate-limited + retried with backoff.
    pub async fn fetch(&self, jamendo_id: &str) -> Result<Fetched> {
        let dest = self.dest(jamendo_id);
        if let Some(f) = self.try_cached(&dest)? {
            return Ok(f);
        }
        tokio::fs::create_dir_all(&self.out_dir).await?;

        let mut attempt = 0u32;
        loop {
            // global token bucket — blocks until a slot is free
            self.limiter.until_ready().await;
            match self.try_once(jamendo_id, &dest).await {
                Ok(f) => return Ok(f),
                Err(e) => {
                    attempt += 1;
                    if attempt > self.max_retries {
                        return Err(anyhow!("giving up on {jamendo_id} after {attempt}: {e}"));
                    }
                    // exponential backoff: 2,4,8,16s
                    let backoff = Duration::from_secs(2u64.saturating_pow(attempt));
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }

    fn try_cached(&self, dest: &Path) -> Result<Option<Fetched>> {
        if let Ok(meta) = std::fs::metadata(dest) {
            if meta.len() > 1024 {
                let bytes = std::fs::read(dest)?;
                return Ok(Some(Fetched {
                    path: dest.to_string_lossy().into_owned(),
                    bytes: meta.len() as i64,
                    sha256: sha_hex(&bytes),
                }));
            }
        }
        Ok(None)
    }

    async fn try_once(&self, jamendo_id: &str, dest: &Path) -> Result<Fetched> {
        let resp = self.client.get(Self::url(jamendo_id)).send().await?;
        let status = resp.status();
        if status.as_u16() == 429 || status.is_server_error() {
            return Err(anyhow!("retryable status {status}"));
        }
        if !status.is_success() {
            // 4xx (e.g. download-disabled tracks): permanent, don't retry-spin
            return Err(anyhow!("permanent status {status}"));
        }
        let body = resp.bytes().await?;
        anyhow::ensure!(body.len() > 1024, "suspiciously small body ({} B)", body.len());
        // atomic write: tmp then rename
        let tmp = dest.with_extension("mp3.part");
        tokio::fs::write(&tmp, &body).await?;
        tokio::fs::rename(&tmp, dest).await?;
        Ok(Fetched {
            path: dest.to_string_lossy().into_owned(),
            bytes: body.len() as i64,
            sha256: sha_hex(&body),
        })
    }
}

fn sha_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
