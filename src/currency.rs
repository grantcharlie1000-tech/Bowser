//! Selection parsing and reference-rate conversions. Only a fixed rates URL leaves the browser.
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

pub const CODES: [&str; 5] = ["GBP", "USD", "EUR", "NZD", "AUD"];
pub const SYMBOLS: [&str; 5] = ["Â£", "$", "â‚¬", "$", "$"];
static TARGET: AtomicUsize = AtomicUsize::new(0);
static CACHE: Mutex<Cache> = Mutex::new(Cache {
    rates: None,
    attempted: None,
    loading: false,
});
struct Cache {
    rates: Option<([f64; 5], Instant)>,
    attempted: Option<Instant>,
    loading: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Price {
    pub cents: u64,
    pub currency: usize,
}

/// A bare dollar sign means USD. Explicit NZ$/A$/ISO codes remove that ambiguity.
pub fn parse(selection: &str) -> Option<Price> {
    let text = selection.trim();
    if text.len() > 80 {
        return None;
    }
    let upper = text.to_uppercase();
    let tokens = [
        ("NZ$", 3),
        ("NZD", 3),
        ("US$", 1),
        ("USD", 1),
        ("AU$", 4),
        ("AUD", 4),
        ("A$", 4),
        ("GBP", 0),
        ("Â£", 0),
        ("EUR", 2),
        ("â‚¬", 2),
        ("$", 1),
    ];
    let (number, currency) = tokens.iter().find_map(|(token, currency)| {
        upper
            .strip_prefix(token)
            .or_else(|| upper.strip_suffix(token))
            .map(|number| (number.trim(), *currency))
    })?;
    let number = number.replace(['\u{a0}', '\u{202f}'], " ");
    let (whole, decimal) = number.split_once('.').unwrap_or((&number, ""));
    if decimal.len() > 2 || !decimal.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let separator = if whole.contains(',') { ',' } else { ' ' };
    let groups: Vec<_> = whole.split(separator).collect();
    if groups[0].is_empty()
        || (groups.len() > 1 && (groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3)))
        || groups
            .iter()
            .any(|g| !g.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let units: u64 = groups.concat().parse().ok()?;
    let fraction = match decimal.len() {
        0 => 0,
        1 => decimal.parse::<u64>().ok()? * 10,
        _ => decimal.parse().ok()?,
    };
    let cents = units.checked_mul(100)?.checked_add(fraction)?;
    (cents <= 100_000_000_000_000).then_some(Price { cents, currency })
}

pub fn target() -> usize {
    TARGET.load(Ordering::Relaxed) % 5
}
pub fn cycle() {
    TARGET.store((target() + 1) % 5, Ordering::Relaxed);
}

/// At most one background request; retry failures after a minute and refresh hourly.
pub fn refresh() {
    let Ok(mut cache) = CACHE.lock() else {
        return;
    };
    if cache.loading
        || cache
            .rates
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(3600))
        || cache
            .attempted
            .is_some_and(|at| at.elapsed() < Duration::from_secs(60))
    {
        return;
    }
    cache.loading = true;
    cache.attempted = Some(Instant::now());
    std::thread::spawn(|| {
        let result = fetch();
        if let Ok(mut cache) = CACHE.lock() {
            if let Some(rates) = result {
                cache.rates = Some((rates, Instant::now()));
            }
            cache.loading = false;
        }
    });
}

fn fetch() -> Option<[f64; 5]> {
    let url =
        url::Url::parse("https://api.frankfurter.dev/v1/latest?base=GBP&symbols=USD,EUR,NZD,AUD")
            .ok()?;
    let mut bytes = Vec::new();
    crate::updater::http::get(&url, 16_384, &mut |chunk| {
        bytes.extend_from_slice(chunk);
        Ok(())
    })
    .ok()?;
    parse_rates(std::str::from_utf8(&bytes).ok()?)
}

fn parse_rates(text: &str) -> Option<[f64; 5]> {
    use crate::extensions::json::{self, Value};
    let Value::Object(root) = json::parse(text)? else {
        return None;
    };
    if json::get(&root, "base") != Some(&Value::String("GBP".into())) {
        return None;
    }
    let Value::Object(values) = json::get(&root, "rates")? else {
        return None;
    };
    let mut rates = [1.0; 5];
    for index in 1..5 {
        let Value::Number(number) = json::get(values, CODES[index])? else {
            return None;
        };
        let rate: f64 = number.parse().ok()?;
        if !rate.is_finite() || rate <= 0.0 {
            return None;
        }
        rates[index] = rate;
    }
    Some(rates)
}

pub fn loading() -> bool {
    CACHE.lock().is_ok_and(|cache| cache.loading)
}

pub fn label(price: Price) -> String {
    let target = target();
    let Ok(cache) = CACHE.lock() else {
        return "Rates unavailable".into();
    };
    if let Some((rates, at)) = &cache.rates {
        if at.elapsed() < Duration::from_secs(86_400) {
            return converted_label(price, target, rates);
        }
    }
    format!(
        "{} Â· {}",
        CODES[target],
        if cache.loading {
            "Loading rateâ€¦"
        } else {
            "Rate unavailable"
        }
    )
}

fn converted_label(price: Price, target: usize, rates: &[f64; 5]) -> String {
    let amount = price.cents as f64 / 100.0 / rates[price.currency] * rates[target];
    let fixed = format!("{amount:.2}");
    let (whole, fraction) = fixed.split_once('.').unwrap_or((&fixed, "00"));
    let mut grouped = String::new();
    for (index, digit) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    format!("â‰ˆ {grouped}.{fraction} {}", CODES[target])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_prices_require_one_unambiguous_amount() {
        for (text, cents, currency) in [
            ("NZ$2,549", 254900, 3),
            ("Â£12.50", 1250, 0),
            ("USD 99.9", 9990, 1),
            ("1 234.56 EUR", 123456, 2),
            ("A$0.05", 5, 4),
            ("$25", 2500, 1),
        ] {
            assert_eq!(parse(text), Some(Price { cents, currency }), "{text}");
        }
        for text in [
            "Price NZ$2,549 today",
            "Â£1,2",
            "$1.234",
            "$NaN",
            "$1e3",
            "$-5",
            "$1,234 567",
            "Â£1 Â£2",
            "2549",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
    }
    #[test]
    fn rates_reject_wrong_base_missing_and_nonpositive_values() {
        let valid = r#"{"base":"GBP","rates":{"USD":1.3,"EUR":1.2,"NZD":2.2,"AUD":2.0}}"#;
        assert_eq!(parse_rates(valid), Some([1.0, 1.3, 1.2, 2.2, 2.0]));
        assert_eq!(parse_rates(&valid.replace("GBP", "USD")), None);
        assert_eq!(parse_rates(&valid.replace("2.2", "0")), None);
        assert_eq!(parse_rates("{}"), None);
    }

    #[test]
    fn cross_rates_round_to_cents_and_group_thousands() {
        let rates = [1.0, 1.25, 1.2, 2.5, 2.0];
        let price = parse("NZ$2,549").unwrap();
        assert_eq!(converted_label(price, 0, &rates), "â‰ˆ 1,019.60 GBP");
        assert_eq!(converted_label(price, 1, &rates), "â‰ˆ 1,274.50 USD");
        assert_eq!(converted_label(price, 3, &rates), "â‰ˆ 2,549.00 NZD");
        assert_eq!(
            converted_label(parse("$0.01").unwrap(), 0, &rates),
            "â‰ˆ 0.01 GBP"
        );
    }
}
