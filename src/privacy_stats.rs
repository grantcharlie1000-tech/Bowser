//! Bounded, host-only statistics for requests the blocker actually stopped.
//!
//! The on-disk format is plain text (`privacy-stats.txt`), contains no URLs, and keeps at most
//! [`MAX_DAYS`] daily buckets and [`MAX_HOSTS`] destination hosts.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub const FILE_NAME: &str = "privacy-stats.txt";
pub const MAX_DAYS: usize = 90;
pub const MAX_HOSTS: usize = 128;
pub const MAX_FILE_BYTES: usize = 64 * 1024;

pub fn today_utc() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrivacyStats {
    all_time: u64,
    days: BTreeMap<String, u64>,
    hosts: BTreeMap<String, u64>,
}

impl PrivacyStats {
    pub fn all_time_total(&self) -> u64 {
        self.all_time
    }

    pub fn day_total(&self, day: &str) -> u64 {
        self.days.get(day).copied().unwrap_or(0)
    }

    pub fn host_total(&self, host: &str) -> u64 {
        self.hosts
            .get(&host.to_ascii_lowercase())
            .copied()
            .unwrap_or(0)
    }

    pub fn days(&self) -> impl DoubleEndedIterator<Item = (&str, u64)> {
        self.days.iter().map(|(day, count)| (day.as_str(), *count))
    }

    pub fn top_sites(&self, limit: usize) -> Vec<(&str, u64)> {
        let mut sites: Vec<_> = self
            .hosts
            .iter()
            .map(|(host, count)| (host.as_str(), *count))
            .collect();
        sites.sort_by(|(host_a, count_a), (host_b, count_b)| {
            count_b.cmp(count_a).then_with(|| host_a.cmp(host_b))
        });
        sites.truncate(limit);
        sites
    }

    pub fn host_count(&self) -> usize {
        self.hosts.len()
    }

    /// Adds one actual blocker decision. Invalid or unavailable site hosts only skip per-site data.
    pub fn record_blocked(&mut self, day: &str, host: &str) {
        if !valid_day(day) {
            return;
        }
        self.all_time = self.all_time.saturating_add(1);
        increment(&mut self.days, day.to_owned());

        let Some(host) = normalise_host(host) else {
            return;
        };

        // A new site when the list is full takes the place of the least blocked one, so the
        // list is never frozen on the first sites ever seen.
        if !self.hosts.contains_key(&host)
            && self.hosts.len() >= MAX_HOSTS
            && let Some(least) = self.hosts.iter().min_by_key(|(name, count)| (**count, std::cmp::Reverse(name.as_str()))).map(|(name, _)| name.clone())
        {
            self.hosts.remove(&least);
        }
        increment(&mut self.hosts, host);

        while self.days.len() > MAX_DAYS {
            if let Some(oldest) = self.days.keys().next().cloned() {
                self.days.remove(&oldest);
            }
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn load(path: &Path) -> Self {
        read_capped(path)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = self.serialise();
        // The timer, the panel and exit all save; only a real change touches the disk.
        if crate::productivity::file_holds(path, text.as_bytes()) {
            return Ok(());
        }
        write_atomic(path, text.as_bytes())
    }

    pub fn parse(text: &str) -> Self {
        if text.len() > MAX_FILE_BYTES {
            return Self::default();
        }
        let mut stats = Self::default();
        for line in text.lines().take(MAX_DAYS + MAX_HOSTS + 8) {
            let mut fields = line.split_whitespace();
            match (fields.next(), fields.next(), fields.next(), fields.next()) {
                (Some("total"), Some(count), None, None) => {
                    if let Ok(count) = count.parse::<u64>() {
                        stats.all_time = count;
                    }
                }
                (Some("day"), Some(day), Some(count), None) if valid_day(day) => {
                    if let Ok(count) = count.parse::<u64>() {
                        stats.days.insert(day.to_owned(), count);
                    }
                }
                (Some("host"), Some(host), Some(count), None) => {
                    if let (Some(host), Ok(count)) = (normalise_host(host), count.parse::<u64>()) {
                        stats.hosts.insert(host, count);
                    }
                }
                _ => {}
            }
        }
        while stats.days.len() > MAX_DAYS {
            let oldest = stats.days.keys().next().unwrap().clone();
            stats.days.remove(&oldest);
        }
        while stats.hosts.len() > MAX_HOSTS {
            let least = stats
                .hosts
                .iter()
                .min_by(|(host_a, count_a), (host_b, count_b)| {
                    count_a.cmp(count_b).then_with(|| host_b.cmp(host_a))
                })
                .map(|(host, _)| host.clone());
            if let Some(host) = least {
                stats.hosts.remove(&host);
            }
        }
        stats
    }

    pub fn serialise(&self) -> String {
        let mut text = format!(
            "# Bowser privacy statistics v1; hosts only, no URLs.\ntotal {}\n",
            self.all_time
        );
        for (day, count) in &self.days {
            text.push_str(&format!("day {day} {count}\n"));
        }
        for (host, count) in &self.hosts {
            text.push_str(&format!("host {host} {count}\n"));
        }
        text
    }
}

fn increment(map: &mut BTreeMap<String, u64>, key: String) {
    let count = map.entry(key).or_default();
    *count = count.saturating_add(1);
}

fn valid_day(day: &str) -> bool {
    let bytes = day.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
        && {
            let year = day[..4].parse::<u16>().unwrap_or(0);
            let month = day[5..7].parse::<u8>().unwrap_or(0);
            let date = day[8..10].parse::<u8>().unwrap_or(0);
            let days_in_month = match month {
                1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                4 | 6 | 9 | 11 => 30,
                2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
                2 => 28,
                _ => 0,
            };
            (1..=days_in_month).contains(&date)
        }
}

fn normalise_host(host: &str) -> Option<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host.len() > 253
        || host.contains('/')
        || host.contains(':')
        || host.contains('@')
        || host.contains('?')
        || host.contains('#')
        || host.chars().any(char::is_whitespace)
    {
        return None;
    }
    Some(host)
}

fn read_capped(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_FILE_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = path.with_extension("txt.tmp");
    let mut file = File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_requests_accumulate_all_time_per_day_and_per_host() {
        let mut stats = PrivacyStats::default();
        stats.record_blocked("2026-10-10", "ads.example");
        stats.record_blocked("2026-10-10", "ads.example");
        stats.record_blocked("2026-10-11", "tracker.example");

        assert_eq!(stats.all_time_total(), 3);
        assert_eq!(stats.day_total("2026-10-10"), 2);
        assert_eq!(stats.day_total("2026-10-11"), 1);
        assert_eq!(stats.host_total("ads.example"), 2);
        assert_eq!(stats.host_total("tracker.example"), 1);
    }

    #[test]
    fn stats_round_trip_as_bounded_human_readable_text() {
        let mut stats = PrivacyStats::default();
        stats.record_blocked("2026-10-11", "tracker.example");
        let decoded = PrivacyStats::parse(&stats.serialise());

        assert_eq!(decoded.all_time_total(), 1);
        assert_eq!(decoded.host_total("tracker.example"), 1);
    }

    #[test]
    fn saving_keeps_only_host_names_and_loads_the_persisted_counters() {
        let path =
            std::env::temp_dir().join(format!("bowser-privacy-stats-{}.txt", std::process::id()));
        let mut stats = PrivacyStats::default();
        stats.record_blocked("2026-10-11", "Tracker.Example");
        stats.save(&path).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        let loaded = PrivacyStats::load(&path);
        let _ = std::fs::remove_file(path);

        assert!(!saved.contains("https://"));
        assert_eq!(loaded.all_time_total(), 1);
        assert_eq!(loaded.host_total("tracker.example"), 1);
    }

    #[test]
    fn corrupt_input_is_ignored_and_counters_saturate() {
        let decoded = PrivacyStats::parse("this is not a privacy stats file\n");
        assert_eq!(decoded.all_time_total(), 0);

        let mut stats = PrivacyStats::parse(&format!("total {}\n", u64::MAX));
        stats.record_blocked("2026-10-11", "tracker.example");
        assert_eq!(stats.all_time_total(), u64::MAX);
    }

    #[test]
    fn retained_days_and_hosts_are_bounded() {
        let mut stats = PrivacyStats::default();
        for day in 0..(MAX_DAYS + 5) {
            stats.record_blocked(
                &format!("2026-{:02}-{:02}", day / 28 + 1, day % 28 + 1),
                "same.example",
            );
        }
        for host in 0..(MAX_HOSTS + 5) {
            stats.record_blocked("2026-10-11", &format!("h{host}.example"));
        }

        assert!(stats.days().count() <= MAX_DAYS);
        assert!(stats.host_count() <= MAX_HOSTS);
    }

    #[test]
    fn a_new_site_is_still_counted_once_the_site_list_is_full() {
        let mut stats = PrivacyStats::default();
        for host in 0..MAX_HOSTS {
            stats.record_blocked("2026-10-11", &format!("h{host}.example"));
        }
        stats.record_blocked("2026-10-11", "h0.example");
        for _ in 0..50 {
            stats.record_blocked("2026-10-11", "new.example");
        }
        assert_eq!(stats.host_total("new.example"), 50, "the current site shows 0 for good");
        assert_eq!(stats.top_sites(5)[0], ("new.example", 50));
        assert_eq!(stats.host_total("h0.example"), 2, "the busier site stays");
        assert_eq!(stats.host_count(), MAX_HOSTS);
    }

    #[test]
    fn a_new_day_rolls_off_the_oldest_daily_bucket_without_changing_all_time() {
        let mut stats = PrivacyStats::default();
        for day in 0..MAX_DAYS {
            stats.record_blocked(
                &format!("2026-{:02}-{:02}", day / 28 + 1, day % 28 + 1),
                "site.example",
            );
        }
        stats.record_blocked("2026-05-01", "site.example");

        assert_eq!(stats.days().count(), MAX_DAYS);
        assert_eq!(stats.day_total("2026-01-01"), 0);
        assert_eq!(stats.day_total("2026-05-01"), 1);
        assert_eq!(stats.all_time_total(), (MAX_DAYS + 1) as u64);
    }

    #[test]
    fn invalid_day_and_non_host_values_are_not_persisted() {
        let mut stats = PrivacyStats::default();
        stats.record_blocked("yesterday", "https://tracker.example/path");
        assert_eq!(stats.all_time_total(), 0);
        stats.record_blocked("2026-10-11", "https://tracker.example/path");
        assert_eq!(stats.all_time_total(), 1);
        assert_eq!(stats.day_total("2026-10-11"), 1);
        assert_eq!(stats.host_count(), 0);
    }

    #[test]
    fn saving_identical_statistics_does_not_rewrite_the_file() {
        let dir = std::env::temp_dir().join(format!("bowser-privacy-stats-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        let mut stats = PrivacyStats::default();
        stats.record_blocked("2026-10-10", "ads.example");
        stats.save(&path).unwrap();
        let stamp = UNIX_EPOCH + std::time::Duration::from_secs(86_400);
        File::options().write(true).open(&path).unwrap().set_modified(stamp).unwrap();
        stats.save(&path).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), stamp, "unchanged bytes are skipped");
        stats.record_blocked("2026-10-10", "ads.example");
        stats.save(&path).unwrap();
        assert_ne!(fs::metadata(&path).unwrap().modified().unwrap(), stamp);
        assert_eq!(PrivacyStats::load(&path).all_time_total(), 2);
        let _ = fs::remove_dir_all(dir);
    }
}
