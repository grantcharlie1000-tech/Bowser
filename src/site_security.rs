//! What the address pill and Site information say about the connection to the current page.
//!
//! Pure mapping from the committed URL and the outcome of the top-level navigation. WebView2 gives no
//! general "secure" flag, so the rules are conservative: a page is only called secure when it is HTTPS
//! and its last top-level navigation finished without an error. HTTP is always "not secure", and
//! anything unclear (loading, a failed load that is not a certificate error, an unknown scheme) is
//! reported as unknown rather than secure.

use url::Url;

/// Outcome of the most recent top-level navigation in a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadState {
    Loading,
    Loaded,
    /// `certificate` is true when WebView2 reported a certificate error for the navigation.
    Failed {
        certificate: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteSecurity {
    /// HTTPS, and the last navigation finished without an error.
    Secure,
    /// Plain HTTP. Never shown as secure.
    NotSecure,
    /// HTTPS, but WebView2 rejected the server certificate for the navigation.
    CertificateProblem,
    /// A local file, a blank page or a browser-internal page.
    Internal,
    /// Loading, a failed load that is not a certificate error, or a scheme Bowser does not classify.
    Unknown,
    /// The tab has no page address yet.
    NoSite,
}

impl SiteSecurity {
    /// Caption for the first line of the Site information panel.
    pub fn summary(self) -> &'static str {
        match self {
            Self::Secure => "Secure connection (HTTPS)",
            Self::NotSecure => "Not secure (HTTP)",
            Self::CertificateProblem => "Certificate problem",
            Self::Internal => "Local or internal page",
            Self::Unknown => "Security status unavailable",
            Self::NoSite => "No site loaded",
        }
    }
}

/// Classifies the connection for the page at `url` (the tab's committed address) given the outcome of
/// its last top-level navigation.
pub fn classify(url: Option<&Url>, load: LoadState) -> SiteSecurity {
    let Some(url) = url else {
        return SiteSecurity::NoSite;
    };
    match url.scheme() {
        "http" => SiteSecurity::NotSecure,
        "https" => match load {
            LoadState::Loaded => SiteSecurity::Secure,
            LoadState::Failed { certificate: true } => SiteSecurity::CertificateProblem,
            LoadState::Loading | LoadState::Failed { certificate: false } => SiteSecurity::Unknown,
        },
        "about" | "file" | "data" | "blob" | "edge" | "chrome" | "bowser" => SiteSecurity::Internal,
        _ => SiteSecurity::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn https_is_secure_only_after_a_clean_load() {
        let page = url("https://example.com/");
        assert_eq!(
            classify(Some(&page), LoadState::Loaded),
            SiteSecurity::Secure
        );
        assert_eq!(
            classify(Some(&page), LoadState::Loading),
            SiteSecurity::Unknown
        );
        assert_eq!(
            classify(Some(&page), LoadState::Failed { certificate: false }),
            SiteSecurity::Unknown
        );
    }

    #[test]
    fn certificate_errors_are_reported_as_problems_never_as_secure() {
        let page = url("https://expired.example/");
        assert_eq!(
            classify(Some(&page), LoadState::Failed { certificate: true }),
            SiteSecurity::CertificateProblem
        );
    }

    #[test]
    fn plain_http_is_never_secure_whatever_the_load_outcome() {
        let page = url("http://example.com/");
        for load in [
            LoadState::Loaded,
            LoadState::Loading,
            LoadState::Failed { certificate: false },
            LoadState::Failed { certificate: true },
        ] {
            assert_eq!(
                classify(Some(&page), load),
                SiteSecurity::NotSecure,
                "{load:?}"
            );
        }
        let local = url("http://localhost:8080/");
        assert_eq!(
            classify(Some(&local), LoadState::Loaded),
            SiteSecurity::NotSecure
        );
    }

    #[test]
    fn local_and_internal_pages_are_labelled_as_such() {
        for text in [
            "file:///C:/Users/charlie/page.html",
            "about:blank",
            "data:text/html,hi",
            "edge://settings",
            "bowser://settings",
            "bowser://settings/privacy",
        ] {
            assert_eq!(
                classify(Some(&url(text)), LoadState::Loaded),
                SiteSecurity::Internal,
                "{text}"
            );
        }
    }

    #[test]
    fn no_address_and_unknown_schemes_are_not_called_secure() {
        assert_eq!(classify(None, LoadState::Loaded), SiteSecurity::NoSite);
        assert_eq!(
            classify(Some(&url("ftp://files.example/")), LoadState::Loaded),
            SiteSecurity::Unknown
        );
    }

    #[test]
    fn every_state_has_a_distinct_caption() {
        let all = [
            SiteSecurity::Secure,
            SiteSecurity::NotSecure,
            SiteSecurity::CertificateProblem,
            SiteSecurity::Internal,
            SiteSecurity::Unknown,
            SiteSecurity::NoSite,
        ];
        for (index, state) in all.iter().enumerate() {
            for other in &all[index + 1..] {
                assert_ne!(state.summary(), other.summary());
            }
        }
    }
}
