//! The value types of the privacy, history and website-appearance settings (`settings.txt` keys
//! `blocker_level`, `history_retention`, `website_appearance` and `private_theme`). They live here
//! so `settings.rs` only stores them; each has a stable `key` for the file and the settings page,
//! and a `parse` that accepts nothing else.

/// `blocker_level=trackers|ads` (default `ads`): what the blocker stops while `blocker_enabled` is
/// on. `Trackers` stops tracking and measurement requests only; `Ads` stops those and ads too.
/// "None" is `blocker_enabled=false`, so the older switch keeps its meaning.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub enum BlockerLevel {
    Trackers,
    #[default]
    Ads,
}

impl BlockerLevel {
    pub fn key(self) -> &'static str {
        match self {
            Self::Trackers => "trackers",
            Self::Ads => "ads",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "trackers" => Some(Self::Trackers),
            "ads" => Some(Self::Ads),
            _ => None,
        }
    }
}

/// `history_retention`: how long Bowser's own history list keeps a visit. `Session` clears the
/// list when Bowser starts and when it closes. The default is three months.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HistoryRetention {
    Session,
    Day,
    Week,
    Month,
    #[default]
    Months3,
    Months6,
    Year,
    Forever,
}

impl HistoryRetention {
    /// Every choice, in the order the settings page lists them.
    pub const ALL: [HistoryRetention; 8] = [
        Self::Session,
        Self::Day,
        Self::Week,
        Self::Month,
        Self::Months3,
        Self::Months6,
        Self::Year,
        Self::Forever,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Day => "1d",
            Self::Week => "1w",
            Self::Month => "1m",
            Self::Months3 => "3m",
            Self::Months6 => "6m",
            Self::Year => "1y",
            Self::Forever => "forever",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|choice| choice.key() == value)
    }

    /// How old a visit may be, in seconds, before it is dropped. `Some(0)` drops everything (a
    /// session-only list), `None` keeps everything.
    pub fn max_age_secs(self) -> Option<u64> {
        const DAY: u64 = 24 * 60 * 60;
        match self {
            Self::Session => Some(0),
            Self::Day => Some(DAY),
            Self::Week => Some(7 * DAY),
            Self::Month => Some(30 * DAY),
            Self::Months3 => Some(91 * DAY),
            Self::Months6 => Some(182 * DAY),
            Self::Year => Some(365 * DAY),
            Self::Forever => None,
        }
    }
}

/// `website_appearance=auto|light|dark` (default `auto`): the colour scheme websites are told the
/// browser prefers (`prefers-color-scheme`). `Auto` follows Windows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WebsiteAppearance {
    #[default]
    Auto,
    Light,
    Dark,
}

impl WebsiteAppearance {
    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

/// `private_theme=follow|light|dark` (default `follow`): the light or dark look of the browser's
/// own surfaces while a private tab is shown. `Follow` keeps the normal appearance.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PrivateTheme {
    #[default]
    Follow,
    Light,
    Dark,
}

impl PrivateTheme {
    /// The appearance the browser's own surfaces take: while a private tab is shown, the one this
    /// theme asks for (`Follow` keeps `normal`); for any other tab, always `normal`.
    pub fn appearance(self, private_tab: bool, normal: crate::theme::Appearance) -> crate::theme::Appearance {
        use crate::theme::Appearance;
        match (private_tab, self) {
            (true, Self::Light) => Appearance::Light,
            (true, Self::Dark) => Appearance::Dark,
            _ => normal,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Follow => "follow",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "follow" => Some(Self::Follow),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_parses_back_to_its_value_and_nothing_else_does() {
        for level in [BlockerLevel::Trackers, BlockerLevel::Ads] {
            assert_eq!(BlockerLevel::parse(level.key()), Some(level));
        }
        for choice in HistoryRetention::ALL {
            assert_eq!(HistoryRetention::parse(choice.key()), Some(choice));
            assert_eq!(HistoryRetention::parse(&choice.key().to_uppercase()), Some(choice));
        }
        for appearance in [WebsiteAppearance::Auto, WebsiteAppearance::Light, WebsiteAppearance::Dark] {
            assert_eq!(WebsiteAppearance::parse(appearance.key()), Some(appearance));
        }
        for theme in [PrivateTheme::Follow, PrivateTheme::Light, PrivateTheme::Dark] {
            assert_eq!(PrivateTheme::parse(theme.key()), Some(theme));
        }
        assert_eq!(BlockerLevel::parse("none"), None, "none is blocker_enabled=false");
        assert_eq!(HistoryRetention::parse("2w"), None);
        assert_eq!(WebsiteAppearance::parse("sepia"), None);
        assert_eq!(PrivateTheme::parse(""), None);
    }

    #[test]
    fn the_private_theme_only_changes_the_look_while_a_private_tab_is_shown() {
        use crate::theme::Appearance::{Dark, Light};
        for normal in [Light, Dark] {
            assert_eq!(PrivateTheme::Follow.appearance(true, normal), normal, "follow keeps the normal look");
            assert_eq!(PrivateTheme::Follow.appearance(false, normal), normal);
            for theme in [PrivateTheme::Light, PrivateTheme::Dark] {
                assert_eq!(theme.appearance(false, normal), normal, "a normal tab never takes the private theme");
            }
        }
        assert_eq!(PrivateTheme::Dark.appearance(true, Light), Dark);
        assert_eq!(PrivateTheme::Light.appearance(true, Dark), Light);
        assert_eq!(PrivateTheme::Dark.appearance(true, Dark), Dark);
    }

    #[test]
    fn retention_ages_grow_and_the_extremes_mean_everything_or_nothing() {
        assert_eq!(HistoryRetention::default(), HistoryRetention::Months3);
        let ages: Vec<_> = HistoryRetention::ALL.iter().map(|choice| choice.max_age_secs()).collect();
        assert_eq!(ages[0], Some(0), "session only drops every visit");
        assert_eq!(*ages.last().unwrap(), None, "forever keeps every visit");
        let finite: Vec<u64> = ages.iter().flatten().copied().collect();
        assert!(finite.windows(2).all(|pair| pair[0] < pair[1]), "{finite:?}");
        assert_eq!(HistoryRetention::Day.max_age_secs(), Some(86_400));
    }
}
