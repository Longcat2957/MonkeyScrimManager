use axum::http::Uri;
use thiserror::Error;

const DEFAULT_NEXT: &str = "/";
const MAX_NEXT_BYTES: usize = 2048;
const ALLOWED_EXACT_PATHS: &[&str] = &["/", "/admin/status", "/docs", "/me", "/records"];
const ALLOWED_PREFIXES: &[&str] = &["/g/", "/users/"];

pub fn validate_next(value: Option<&str>) -> Result<String, NextPathError> {
    let Some(value) = value else {
        return Ok(DEFAULT_NEXT.to_owned());
    };

    if value.is_empty()
        || value.len() > MAX_NEXT_BYTES
        || value.starts_with("//")
        || value.contains('\\')
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(NextPathError);
    }

    let uri = value.parse::<Uri>().map_err(|_| NextPathError)?;
    if uri.scheme().is_some() || uri.authority().is_some() || !uri.path().starts_with('/') {
        return Err(NextPathError);
    }

    let path = uri.path();
    if !ALLOWED_EXACT_PATHS.contains(&path)
        && !ALLOWED_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix) && path.len() > prefix.len())
    {
        return Err(NextPathError);
    }

    Ok(value.to_owned())
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("next must be an allowlisted same-origin application path")]
pub struct NextPathError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_known_frontend_destinations() {
        for value in [
            None,
            Some("/"),
            Some("/me?tab=privacy"),
            Some("/g/123456789012345678/series?status=active"),
            Some("/users/123456789012345678"),
            Some("/admin/status"),
        ] {
            assert!(validate_next(value).is_ok(), "rejected {value:?}");
        }
    }

    #[test]
    fn rejects_open_redirects_and_unknown_backend_paths() {
        for value in [
            "",
            "https://example.com/",
            "//example.com/",
            "/\\example.com/",
            "/api/me",
            "/oauth/callback",
            "/ws",
            "/g/",
            "/users/",
            "/unknown",
            "/me\r\nLocation: https://example.com",
        ] {
            assert!(validate_next(Some(value)).is_err(), "accepted {value:?}");
        }
        assert!(validate_next(Some(&format!("/g/{}", "a".repeat(2048)))).is_err());
    }
}
