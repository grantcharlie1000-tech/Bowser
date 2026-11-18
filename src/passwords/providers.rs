//! Which password manager or browser wrote a CSV export, and which of its columns hold the site,
//! username and password.
//!
//! Every exporter names its columns differently (`login_uri` in Bitwarden, `Pwd` in RoboForm,
//! `Website Address` elsewhere), and some leave the site without a scheme (`example.com`). The
//! headers are normalised (`normalize_header`: lowercase ASCII letters and digits only) before
//! they reach this module, so `Login URI`, `login_uri` and `LoginUri` are the same column.

/// A recognised exporter. `label` is shown on the import preview ("Bitwarden export").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Provider {
    pub id: &'static str,
    pub label: &'static str,
}

pub const GENERIC: Provider = Provider { id: "csv", label: "Password CSV" };

/// Each provider with the normalised headers that identify it. The first entry whose headers are
/// all present wins, so the more specific signatures come first.
const SIGNATURES: &[(Provider, &[&str])] = &[
    (Provider { id: "bitwarden", label: "Bitwarden" }, &["loginuri", "loginusername", "loginpassword"]),
    (Provider { id: "1password", label: "1Password" }, &["title", "url", "username", "password", "otpauth", "favorite"]),
    (Provider { id: "1password", label: "1Password" }, &["title", "url", "username", "password", "archived"]),
    (Provider { id: "apple", label: "Apple Passwords or Safari" }, &["title", "url", "username", "password", "otpauth"]),
    (Provider { id: "lastpass", label: "LastPass" }, &["url", "username", "password", "extra", "name", "grouping"]),
    (Provider { id: "dashlane", label: "Dashlane" }, &["username", "username2", "title", "password", "url"]),
    (Provider { id: "dashlane", label: "Dashlane" }, &["username", "title", "password", "url", "otpsecret"]),
    (Provider { id: "firefox", label: "Firefox" }, &["url", "username", "password", "httprealm", "formactionorigin"]),
    (Provider { id: "nordpass", label: "NordPass" }, &["name", "url", "username", "password", "cardholdername"]),
    (Provider { id: "protonpass", label: "Proton Pass" }, &["type", "name", "url", "email", "username", "password", "vault"]),
    (Provider { id: "keepass", label: "KeePass or KeePassXC" }, &["group", "title", "username", "password", "url"]),
    (Provider { id: "keepass", label: "KeePass or KeePassXC" }, &["account", "loginname", "password", "website"]),
    (Provider { id: "roboform", label: "RoboForm" }, &["name", "url", "matchurl", "login", "pwd"]),
    (Provider { id: "enpass", label: "Enpass" }, &["title", "username", "email", "password", "website"]),
    (Provider { id: "keeper", label: "Keeper" }, &["folder", "title", "login", "password", "websiteaddress"]),
    (Provider { id: "zoho", label: "Zoho Vault" }, &["passwordname", "passwordurl", "username", "password"]),
    (Provider { id: "chromium", label: "Chrome, Edge, Brave, Opera or Vivaldi" }, &["name", "url", "username", "password"]),
];

/// Column names that hold the sign-in page, in order of preference.
pub const URL_COLUMNS: &[&str] = &[
    "url", "loginuri", "loginurl", "uri", "website", "websiteaddress", "websiteurl", "weburl", "webaddress",
    "siteurl", "site", "passwordurl", "hostname", "origin", "originurl", "matchurl", "domain",
];
/// Column names that hold the username, in order of preference. A row whose first username column
/// is empty uses the next non-empty one (Proton Pass and Enpass keep email and username apart).
pub const USER_COLUMNS: &[&str] = &[
    "username", "loginusername", "user", "login", "loginname", "userid", "account", "email", "emailaddress",
    "loginemail", "username1", "username2",
];
pub const PASSWORD_COLUMNS: &[&str] = &["password", "loginpassword", "pass", "pwd", "passwd", "passwordvalue"];

/// The exporter whose signature the headers carry, or the generic label.
pub fn detect(headers: &[String]) -> Provider {
    SIGNATURES
        .iter()
        .find(|(_, needed)| needed.iter().all(|name| headers.iter().any(|header| header == name)))
        .map_or(GENERIC, |(provider, _)| *provider)
}

/// Every column of `headers` named in `aliases`, in the aliases' order of preference.
pub fn columns(headers: &[String], aliases: &[&str]) -> Vec<usize> {
    aliases
        .iter()
        .filter_map(|alias| headers.iter().position(|header| header == alias))
        .collect()
}

/// The first non-empty, trimmed value among `columns` of `row`.
pub fn first_value<'a>(row: &'a [String], columns: &[usize]) -> &'a str {
    columns
        .iter()
        .filter_map(|&column| row.get(column).map(|value| value.trim()))
        .find(|value| !value.is_empty())
        .unwrap_or("")
}

/// Gives a schemeless site the HTTPS scheme the vault requires (`example.com/login` becomes
/// `https://example.com/login`). A value that already names a scheme is left as it is, so
/// `http://`, `androidapp://` and the like are still judged (and refused) by the vault's own rules.
pub fn with_scheme(value: &str) -> String {
    let value = value.trim();
    if value.contains("://") {
        // Several managers allow more than one address per login; the first one is the sign-in page.
        return value.split_whitespace().next().unwrap_or("").to_owned();
    }
    // Words with spaces ("Secure note") are a label, not a site.
    if value.is_empty() || value.contains(char::is_whitespace) {
        return value.to_owned();
    }
    let host = value.split(['/', '?', '#']).next().unwrap_or("");
    let looks_like_host = host.contains('.') || host.starts_with("localhost");
    if looks_like_host && !host.contains('@') && !host.contains(':') || host.starts_with("localhost:") {
        let scheme = if host.starts_with("localhost") { "http" } else { "https" };
        format!("{scheme}://{value}")
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(line: &str) -> Vec<String> {
        line.split(',')
            .map(|h| h.chars().filter(char::is_ascii_alphanumeric).flat_map(char::to_lowercase).collect())
            .collect()
    }

    #[test]
    fn each_exporter_is_recognised_by_its_own_headers() {
        let cases = [
            ("folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp", "bitwarden"),
            ("Title,Url,Username,Password,OTPAuth,Favorite,Archived,Tags,Notes", "1password"),
            ("Title,URL,Username,Password,Notes,OTPAuth", "apple"),
            ("url,username,password,totp,extra,name,grouping,fav", "lastpass"),
            ("username,username2,username3,title,password,note,url,category,otpSecret", "dashlane"),
            ("url,username,password,httpRealm,formActionOrigin,guid,timeCreated,timeLastUsed,timePasswordChanged", "firefox"),
            ("name,url,username,password,note,cardholdername,cardnumber,cvc,expirydate,zipcode,folder,full_name,phone_number,email,address1,address2,city,country,state,type", "nordpass"),
            ("type,name,url,email,username,password,note,totp,createTime,modifyTime,vault", "protonpass"),
            ("Group,Title,Username,Password,URL,Notes,TOTP,Icon,Last Modified,Created", "keepass"),
            ("Name,Url,MatchUrl,Login,Pwd,Note,Folder,RfFieldsV2", "roboform"),
            ("name,url,username,password,note", "chromium"),
            ("name,url,username,password", "chromium"),
            ("site,user,pass", "csv"),
        ];
        for (line, id) in cases {
            assert_eq!(detect(&headers(line)).id, id, "{line}");
        }
    }

    #[test]
    fn columns_follow_preference_and_rows_fall_back_to_the_next_non_empty_value() {
        let h = headers("type,name,url,email,username,password");
        let users = columns(&h, USER_COLUMNS);
        assert_eq!(users, vec![4, 3]);
        let row: Vec<String> = ["login", "Example", "example.com", "me@example.com", "", "pw"].map(String::from).to_vec();
        assert_eq!(first_value(&row, &users), "me@example.com");
        assert_eq!(columns(&headers("Name,Url,MatchUrl,Login,Pwd"), PASSWORD_COLUMNS), vec![4]);
        assert_eq!(columns(&headers("Name,Url,MatchUrl,Login,Pwd"), URL_COLUMNS), vec![1, 2]);
    }

    #[test]
    fn schemeless_sites_become_https_and_others_are_left_for_the_vault_to_judge() {
        assert_eq!(with_scheme("example.com"), "https://example.com");
        assert_eq!(with_scheme(" accounts.example.co.uk/login?x=1 "), "https://accounts.example.co.uk/login?x=1");
        assert_eq!(with_scheme("localhost:8080/login"), "http://localhost:8080/login");
        assert_eq!(with_scheme("https://a.example https://b.example"), "https://a.example");
        assert_eq!(with_scheme("http://example.com"), "http://example.com");
        assert_eq!(with_scheme("androidapp://com.example"), "androidapp://com.example");
        assert_eq!(with_scheme("Secure note"), "Secure note");
        assert_eq!(with_scheme("me@example.com"), "me@example.com");
        assert_eq!(with_scheme(""), "");
    }
}
