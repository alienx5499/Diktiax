//! Per-visitor cooldown on `POST /api/investigate/{id}`, the only route that calls the LLM.

use axum::http::HeaderMap;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// One investigation per visitor per this many seconds. `INVESTIGATE_COOLDOWN_SECS` overrides it
/// (0 disables) for local dev and tests.
pub const INVESTIGATE_COOLDOWN_SECS: u64 = 15;

/// One `ring_components` call per visitor per this many seconds over HTTP (whole-graph algo.WCC).
pub const RING_COMPONENTS_COOLDOWN_SECS: u64 = 3;

/// Prune expired entries once the map grows past this, so memory stays bounded.
const PRUNE_THRESHOLD: usize = 1_000;
/// Hard cap on tracked visitors, even if all of them are still inside their cooldown.
const MAX_TRACKED: usize = 10_000;

// In-memory, single instance; use a shared store if the backend scales out.
pub struct Cooldown {
    period: Duration,
    last_accepted: Mutex<HashMap<String, Instant>>,
}

impl Cooldown {
    pub fn new(secs: u64) -> Self {
        Self {
            period: Duration::from_secs(secs),
            last_accepted: Mutex::new(HashMap::new()),
        }
    }

    pub fn from_env() -> Self {
        let secs = std::env::var("INVESTIGATE_COOLDOWN_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(INVESTIGATE_COOLDOWN_SECS);
        Self::new(secs)
    }

    pub fn period_secs(&self) -> u64 {
        self.period.as_secs()
    }

    /// `Err(seconds to wait, rounded up)` when `visitor` is still cooling down.
    pub fn check(&self, visitor: &str) -> Result<(), u64> {
        let mut map = self
            .last_accepted
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        check_at(&mut map, visitor, Instant::now(), self.period)
    }
}

/// Pure limiter step with an injectable clock. Records `now` only when the request is accepted,
/// so a rejected retry doesn't extend the wait.
fn check_at(
    map: &mut HashMap<String, Instant>,
    visitor: &str,
    now: Instant,
    period: Duration,
) -> Result<(), u64> {
    if period.is_zero() {
        return Ok(());
    }
    if let Some(&last) = map.get(visitor) {
        let elapsed = now.saturating_duration_since(last);
        if elapsed < period {
            let wait = period - elapsed;
            return Err(wait.as_secs() + u64::from(wait.subsec_nanos() > 0));
        }
    }
    if map.len() > PRUNE_THRESHOLD {
        map.retain(|_, t| now.saturating_duration_since(*t) < period);
    }
    if map.len() >= MAX_TRACKED {
        // Evict only the oldest entry: active visitors keep their cooldown and nobody is refused.
        // O(n) scan, only reached under a flood of 10k+ distinct visitors in one window.
        if let Some(oldest) = map.iter().min_by_key(|(_, t)| **t).map(|(k, _)| k.clone()) {
            map.remove(&oldest);
        }
    }
    map.insert(visitor.to_owned(), now);
    Ok(())
}

/// `CF-Connecting-IP`, else the peer address, else "unknown".
/// Render sits behind Cloudflare, which sets `CF-Connecting-IP` to the real client address and
/// overwrites any value the client sends, so a visitor can't pick another bucket or dodge their own.
/// `X-Forwarded-For` is ignored: Render appends to whatever the client sent, so its entries are
/// client-controlled. Locally (no Cloudflare) the TCP peer address is used. A header value that
/// isn't a valid IP is ignored.
pub fn visitor_key(headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
    headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<IpAddr>().ok())
        .or_else(|| peer.map(|p| p.ip()))
        .map(bucket)
        .unwrap_or_else(|| "unknown".to_owned())
}

/// IPv4 per address; IPv6 per /64, since one subscriber usually controls a whole /64.
fn bucket(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let s = v6.segments();
                format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: Duration = Duration::from_secs(INVESTIGATE_COOLDOWN_SECS);

    #[test]
    fn one_request_per_visitor_per_period() {
        let mut map = HashMap::new();
        let t0 = Instant::now();
        assert_eq!(check_at(&mut map, "a", t0, PERIOD), Ok(()));
        assert_eq!(
            check_at(&mut map, "a", t0 + Duration::from_millis(5_500), PERIOD),
            Err(10),
            "9.5 s left rounds up"
        );
        assert_eq!(
            check_at(&mut map, "a", t0 + Duration::from_secs(6), PERIOD),
            Err(9)
        );
        assert_eq!(
            check_at(&mut map, "b", t0 + Duration::from_secs(1), PERIOD),
            Ok(()),
            "visitors are independent"
        );
        assert_eq!(
            check_at(&mut map, "a", t0 + PERIOD, PERIOD),
            Ok(()),
            "rejections did not extend the wait"
        );
    }

    #[test]
    fn zero_disables() {
        let mut map = HashMap::new();
        let t0 = Instant::now();
        assert_eq!(check_at(&mut map, "a", t0, Duration::ZERO), Ok(()));
        assert_eq!(check_at(&mut map, "a", t0, Duration::ZERO), Ok(()));
    }

    #[test]
    fn prunes_expired_entries() {
        let mut map = HashMap::new();
        let t0 = Instant::now();
        for i in 0..=PRUNE_THRESHOLD {
            check_at(&mut map, &i.to_string(), t0, PERIOD).unwrap();
        }
        check_at(&mut map, "late", t0 + PERIOD, PERIOD).unwrap();
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn visitor_key_trusts_only_cloudflare_header() {
        let peer: SocketAddr = "10.0.0.9:5555".parse().unwrap();
        let mut h = HeaderMap::new();
        assert_eq!(visitor_key(&h, None), "unknown");
        assert_eq!(visitor_key(&h, Some(peer)), "10.0.0.9");
        // client-supplied forwarding headers must not change the bucket
        h.insert("x-forwarded-for", "1.1.1.1".parse().unwrap());
        h.insert("x-real-ip", "2.2.2.2".parse().unwrap());
        assert_eq!(visitor_key(&h, Some(peer)), "10.0.0.9");
        h.insert("cf-connecting-ip", " 203.0.113.7 ".parse().unwrap());
        assert_eq!(visitor_key(&h, Some(peer)), "203.0.113.7");
    }

    #[test]
    fn ipv6_is_grouped_per_64_and_bad_header_ignored() {
        let peer: SocketAddr = "10.0.0.9:5555".parse().unwrap();
        let mut h = HeaderMap::new();
        h.insert("cf-connecting-ip", "2001:db8:1:2:aaaa::1".parse().unwrap());
        let a = visitor_key(&h, Some(peer));
        h.insert("cf-connecting-ip", "2001:db8:1:2:bbbb::9".parse().unwrap());
        assert_eq!(visitor_key(&h, Some(peer)), a, "same /64, same bucket");
        assert_eq!(a, "2001:db8:1:2::/64");
        h.insert("cf-connecting-ip", "not-an-ip".parse().unwrap());
        assert_eq!(visitor_key(&h, Some(peer)), "10.0.0.9");
        h.insert("cf-connecting-ip", "::ffff:198.51.100.4".parse().unwrap());
        assert_eq!(visitor_key(&h, Some(peer)), "198.51.100.4");
    }

    #[test]
    fn map_never_exceeds_the_hard_cap() {
        let mut map = HashMap::new();
        let t0 = Instant::now();
        for i in 0..(MAX_TRACKED + 50) {
            check_at(&mut map, &i.to_string(), t0, PERIOD).unwrap();
            assert!(map.len() <= MAX_TRACKED);
        }
    }

    #[test]
    fn full_map_keeps_active_visitors_throttled() {
        let mut map = HashMap::new();
        let t0 = Instant::now();
        check_at(&mut map, "active", t0 + Duration::from_millis(5), PERIOD).unwrap();
        for i in 0..MAX_TRACKED {
            check_at(
                &mut map,
                &i.to_string(),
                t0 + Duration::from_millis(10),
                PERIOD,
            )
            .unwrap();
        }
        assert!(map.len() <= MAX_TRACKED);
        // a visitor admitted while the map is full is still throttled on its next request
        check_at(&mut map, "fresh", t0 + Duration::from_millis(20), PERIOD).unwrap();
        assert!(
            check_at(&mut map, "fresh", t0 + Duration::from_millis(30), PERIOD).is_err(),
            "still throttled"
        );
    }
}
