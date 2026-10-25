use std::borrow::Cow;

/// Splits an `http(s)` URL into (lowercased host, path+query). `None` for other schemes,
/// IPv6 literals and malformed input. Allocates only when the host contains uppercase.
pub(crate) fn split_url(url: &str) -> Option<(Cow<'_, str>, &str)> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| {
            let (scheme, rest) = url.split_once("://")?;
            (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
                .then_some(rest)
        })?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let authority = authority.rsplit('@').next()?;
    if authority.starts_with('[') {
        return None;
    }
    let host = authority.split(':').next()?;
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() {
        return None;
    }
    let tail = tail.split('#').next().unwrap_or("");
    let path = if tail.is_empty() { "/" } else { tail };
    let host = if host.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(host.to_ascii_lowercase())
    } else {
        Cow::Borrowed(host)
    };
    Some((host, path))
}
