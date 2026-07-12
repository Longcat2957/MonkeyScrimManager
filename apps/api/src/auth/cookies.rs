use std::time::Duration;

use axum::http::{HeaderMap, HeaderName};
use cookie::{Cookie, SameSite, time::Duration as CookieDuration};

pub const SESSION_COOKIE_NAME: &str = "monkeybot_session";
pub const CSRF_COOKIE_NAME: &str = "monkeybot_csrf";
pub const OAUTH_STATE_COOKIE_NAME: &str = "monkeybot_oauth_state";
pub const CSRF_HEADER_NAME: HeaderName = HeaderName::from_static("x-csrf-token");

const OAUTH_STATE_TTL: Duration = Duration::from_secs(5 * 60);
const OAUTH_CALLBACK_PATH: &str = "/oauth/callback";

#[derive(Clone, Debug)]
pub struct CookiePolicy {
    secure: bool,
    session_ttl: Duration,
}

impl CookiePolicy {
    pub const fn new(secure: bool, session_ttl: Duration) -> Self {
        Self {
            secure,
            session_ttl,
        }
    }

    pub const fn session_ttl(&self) -> Duration {
        self.session_ttl
    }

    pub fn set_session(&self, value: &str) -> String {
        self.build(SESSION_COOKIE_NAME, value, "/", true, self.session_ttl)
    }

    pub fn clear_session(&self) -> String {
        self.clear(SESSION_COOKIE_NAME, "/", true)
    }

    pub fn set_csrf(&self, value: &str) -> String {
        self.build(CSRF_COOKIE_NAME, value, "/", false, self.session_ttl)
    }

    pub fn clear_csrf(&self) -> String {
        self.clear(CSRF_COOKIE_NAME, "/", false)
    }

    pub fn set_oauth_state(&self, value: &str) -> String {
        self.build(
            OAUTH_STATE_COOKIE_NAME,
            value,
            OAUTH_CALLBACK_PATH,
            true,
            OAUTH_STATE_TTL,
        )
    }

    pub fn clear_oauth_state(&self) -> String {
        self.clear(OAUTH_STATE_COOKIE_NAME, OAUTH_CALLBACK_PATH, true)
    }

    fn build(
        &self,
        name: &'static str,
        value: &str,
        path: &'static str,
        http_only: bool,
        ttl: Duration,
    ) -> String {
        Cookie::build((name, value.to_owned()))
            .path(path)
            .http_only(http_only)
            .secure(self.secure)
            .same_site(SameSite::Lax)
            .max_age(cookie_duration(ttl))
            .build()
            .to_string()
    }

    fn clear(&self, name: &'static str, path: &'static str, http_only: bool) -> String {
        Cookie::build((name, String::new()))
            .path(path)
            .http_only(http_only)
            .secure(self.secure)
            .same_site(SameSite::Lax)
            .max_age(CookieDuration::ZERO)
            .build()
            .to_string()
    }
}

pub fn parse_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut found = None;
    for header in headers.get_all(axum::http::header::COOKIE) {
        let header = header.to_str().ok()?;
        for pair in header.split(';') {
            let Ok(cookie) = Cookie::parse(pair.trim().to_owned()) else {
                continue;
            };
            if cookie.name() != name {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = Some(cookie.value().to_owned());
        }
    }
    found
}

fn cookie_duration(duration: Duration) -> CookieDuration {
    CookieDuration::seconds(i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::*;

    #[test]
    fn builds_host_only_session_and_csrf_cookies() {
        let policy = CookiePolicy::new(true, Duration::from_secs(3600));
        let session = policy.set_session("session-token");
        let csrf = policy.set_csrf("csrf-token");

        assert!(session.contains("monkeybot_session=session-token"));
        assert!(session.contains("HttpOnly"));
        assert!(session.contains("Secure"));
        assert!(session.contains("SameSite=Lax"));
        assert!(session.contains("Path=/"));
        assert!(session.contains("Max-Age=3600"));
        assert!(!session.contains("Domain="));

        assert!(csrf.contains("monkeybot_csrf=csrf-token"));
        assert!(!csrf.contains("HttpOnly"));
        assert!(csrf.contains("Secure"));
    }

    #[test]
    fn oauth_cookie_is_scoped_to_the_callback_and_cleared_with_the_same_path() {
        let policy = CookiePolicy::new(false, Duration::from_secs(3600));
        let set = policy.set_oauth_state("state");
        let clear = policy.clear_oauth_state();

        assert!(set.contains("Path=/oauth/callback"));
        assert!(set.contains("Max-Age=300"));
        assert!(set.contains("HttpOnly"));
        assert!(!set.contains("Secure"));
        assert!(clear.contains("Path=/oauth/callback"));
        assert!(clear.contains("Max-Age=0"));
    }

    #[test]
    fn parses_cookie_headers_without_accepting_invalid_segments() {
        let mut headers = HeaderMap::new();
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("ignored; monkeybot_session=abc_123"),
        );
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("monkeybot_csrf=csrf-token"),
        );

        assert_eq!(
            parse_cookie(&headers, SESSION_COOKIE_NAME).as_deref(),
            Some("abc_123")
        );
        assert_eq!(
            parse_cookie(&headers, CSRF_COOKIE_NAME).as_deref(),
            Some("csrf-token")
        );
        assert_eq!(parse_cookie(&headers, "missing"), None);
    }

    #[test]
    fn rejects_ambiguous_duplicate_security_cookies() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("monkeybot_session=first; monkeybot_session=second"),
        );

        assert_eq!(parse_cookie(&headers, SESSION_COOKIE_NAME), None);
    }
}
