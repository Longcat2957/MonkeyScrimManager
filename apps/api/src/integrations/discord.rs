use std::time::Duration;

use futures_util::future::BoxFuture;
use reqwest::{Client, StatusCode, Url};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use thiserror::Error;

use crate::domain::DiscordUserId;

const DISCORD_AUTHORIZE_URL: &str = "https://discord.com/oauth2/authorize";
const DISCORD_TOKEN_URL: &str = "https://discord.com/api/v10/oauth2/token";
const DISCORD_CURRENT_USER_URL: &str = "https://discord.com/api/v10/users/@me";
const DISCORD_CDN_URL: &str = "https://cdn.discordapp.com";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const USER_AGENT: &str = "MonkeyBot/0.1 (+https://monkeybot.lol)";

pub trait DiscordClient: Send + Sync {
    fn authorization_url(
        &self,
        state: &str,
        pkce_challenge: &str,
    ) -> Result<Url, DiscordClientError>;

    fn authenticate<'a>(
        &'a self,
        code: &'a str,
        pkce_verifier: &'a SecretString,
    ) -> BoxFuture<'a, Result<DiscordUser, DiscordClientError>>;
}

#[derive(Clone)]
pub struct ReqwestDiscordClient {
    http: Client,
    client_id: String,
    client_secret: SecretString,
    redirect_uri: Url,
}

impl ReqwestDiscordClient {
    pub fn new(
        client_id: String,
        client_secret: SecretString,
        redirect_uri: Url,
    ) -> Result<Self, DiscordClientError> {
        if !is_snowflake(&client_id) || !is_http_redirect(&redirect_uri) {
            return Err(DiscordClientError::InvalidConfiguration);
        }
        let http = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .map_err(DiscordClientError::Request)?;
        Ok(Self {
            http,
            client_id,
            client_secret,
            redirect_uri,
        })
    }

    async fn authenticate_inner(
        &self,
        code: &str,
        pkce_verifier: &SecretString,
    ) -> Result<DiscordUser, DiscordClientError> {
        if code.is_empty() || code.len() > 2048 {
            return Err(DiscordClientError::InvalidAuthorizationCode);
        }

        let token_response = self
            .http
            .post(DISCORD_TOKEN_URL)
            .basic_auth(&self.client_id, Some(self.client_secret.expose_secret()))
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.redirect_uri.as_str()),
                ("code_verifier", pkce_verifier.expose_secret()),
            ])
            .send()
            .await
            .map_err(DiscordClientError::Request)?;
        ensure_success(OAuthStage::TokenExchange, token_response.status())?;
        let token = token_response
            .json::<DiscordTokenResponse>()
            .await
            .map_err(|_| DiscordClientError::MalformedResponse(OAuthStage::TokenExchange))?;
        token.validate()?;

        let user_response = self
            .http
            .get(DISCORD_CURRENT_USER_URL)
            .bearer_auth(token.access_token.expose_secret())
            .send()
            .await
            .map_err(DiscordClientError::Request)?;
        ensure_success(OAuthStage::CurrentUser, user_response.status())?;
        let raw_user = user_response
            .json::<DiscordUserResponse>()
            .await
            .map_err(|_| DiscordClientError::MalformedResponse(OAuthStage::CurrentUser))?;
        DiscordUser::try_from(raw_user)
    }
}

impl DiscordClient for ReqwestDiscordClient {
    fn authorization_url(
        &self,
        state: &str,
        pkce_challenge: &str,
    ) -> Result<Url, DiscordClientError> {
        if !is_oauth_nonce(state) || !is_oauth_nonce(pkce_challenge) {
            return Err(DiscordClientError::InvalidOAuthMaterial);
        }

        let mut url = Url::parse(DISCORD_AUTHORIZE_URL)
            .map_err(|_| DiscordClientError::InvalidConfiguration)?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", self.redirect_uri.as_str())
            .append_pair("scope", "identify")
            .append_pair("state", state)
            .append_pair("code_challenge", pkce_challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url)
    }

    fn authenticate<'a>(
        &'a self,
        code: &'a str,
        pkce_verifier: &'a SecretString,
    ) -> BoxFuture<'a, Result<DiscordUser, DiscordClientError>> {
        Box::pin(self.authenticate_inner(code, pkce_verifier))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscordUser {
    pub id: DiscordUserId,
    pub display_name: String,
    pub avatar_hash: Option<String>,
}

impl DiscordUser {
    pub fn avatar_url(&self) -> Option<String> {
        self.avatar_hash.as_ref().map(|avatar_hash| {
            format!(
                "{DISCORD_CDN_URL}/avatars/{}/{avatar_hash}.webp?size=128",
                self.id.as_str()
            )
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OAuthStage {
    TokenExchange,
    CurrentUser,
}

#[derive(Debug, Error)]
pub enum DiscordClientError {
    #[error("Discord OAuth client configuration is invalid")]
    InvalidConfiguration,
    #[error("OAuth state or PKCE challenge is invalid")]
    InvalidOAuthMaterial,
    #[error("Discord authorization code is invalid")]
    InvalidAuthorizationCode,
    #[error("Discord request failed")]
    Request(#[source] reqwest::Error),
    #[error("Discord rejected the {stage:?} request with HTTP {status}")]
    Rejected { stage: OAuthStage, status: u16 },
    #[error("Discord returned a malformed {0:?} response")]
    MalformedResponse(OAuthStage),
    #[error("Discord token response did not grant identify access")]
    InvalidToken,
    #[error("Discord returned an invalid user identity")]
    InvalidUser,
}

#[derive(Deserialize)]
struct DiscordTokenResponse {
    access_token: SecretString,
    token_type: String,
    scope: String,
}

impl DiscordTokenResponse {
    fn validate(&self) -> Result<(), DiscordClientError> {
        let identify_granted = self
            .scope
            .split_ascii_whitespace()
            .any(|scope| scope == "identify");
        if self.access_token.expose_secret().is_empty()
            || self.access_token.expose_secret().len() > 4096
            || !self.token_type.eq_ignore_ascii_case("bearer")
            || !identify_granted
        {
            return Err(DiscordClientError::InvalidToken);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct DiscordUserResponse {
    id: String,
    username: String,
    global_name: Option<String>,
    avatar: Option<String>,
}

impl TryFrom<DiscordUserResponse> for DiscordUser {
    type Error = DiscordClientError;

    fn try_from(value: DiscordUserResponse) -> Result<Self, Self::Error> {
        let id = DiscordUserId::parse(value.id).map_err(|_| DiscordClientError::InvalidUser)?;
        let display_name = value
            .global_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(value.username);
        if display_name.trim().is_empty() || display_name.len() > 128 {
            return Err(DiscordClientError::InvalidUser);
        }

        let avatar_hash = value.avatar.filter(|hash| !hash.is_empty());
        if avatar_hash
            .as_deref()
            .is_some_and(|hash| !is_avatar_hash(hash))
        {
            return Err(DiscordClientError::InvalidUser);
        }

        Ok(Self {
            id,
            display_name,
            avatar_hash,
        })
    }
}

fn ensure_success(stage: OAuthStage, status: StatusCode) -> Result<(), DiscordClientError> {
    if status.is_success() {
        Ok(())
    } else {
        Err(DiscordClientError::Rejected {
            stage,
            status: status.as_u16(),
        })
    }
}

fn is_http_redirect(value: &Url) -> bool {
    matches!(value.scheme(), "http" | "https")
        && value.host_str().is_some()
        && value.username().is_empty()
        && value.password().is_none()
        && value.fragment().is_none()
        && value.query().is_none()
        && value.path() == "/oauth/callback"
}

fn is_snowflake(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('0')
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

fn is_oauth_nonce(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
}

fn is_avatar_hash(value: &str) -> bool {
    value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn client() -> ReqwestDiscordClient {
        ReqwestDiscordClient::new(
            "123456789012345678".to_owned(),
            SecretString::from("client-secret".to_owned()),
            Url::parse("https://dev.monkeybot.lol/oauth/callback").expect("redirect URI"),
        )
        .expect("client")
    }

    #[test]
    fn authorization_url_contains_state_pkce_and_minimal_scope() {
        let state = "a".repeat(43);
        let challenge = "b".repeat(43);
        let url = client()
            .authorization_url(&state, &challenge)
            .expect("authorization URL");
        let query = url.query_pairs().into_owned().collect::<HashMap<_, _>>();

        assert_eq!(url.as_str().split('?').next(), Some(DISCORD_AUTHORIZE_URL));
        assert_eq!(query.get("response_type").map(String::as_str), Some("code"));
        assert_eq!(
            query.get("client_id").map(String::as_str),
            Some("123456789012345678")
        );
        assert_eq!(query.get("scope").map(String::as_str), Some("identify"));
        assert_eq!(query.get("state").map(String::as_str), Some(state.as_str()));
        assert_eq!(
            query.get("code_challenge").map(String::as_str),
            Some(challenge.as_str())
        );
        assert_eq!(
            query.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
    }

    #[test]
    fn rejects_non_callback_redirects_and_invalid_oauth_material() {
        assert!(matches!(
            ReqwestDiscordClient::new(
                "123456789012345678".to_owned(),
                SecretString::from("secret".to_owned()),
                Url::parse("https://dev.monkeybot.lol/other").expect("URI"),
            ),
            Err(DiscordClientError::InvalidConfiguration)
        ));
        assert!(matches!(
            client().authorization_url("short", &"b".repeat(43)),
            Err(DiscordClientError::InvalidOAuthMaterial)
        ));
    }

    #[test]
    fn maps_discord_profile_without_persisting_oauth_tokens() {
        let user = DiscordUser::try_from(DiscordUserResponse {
            id: "123456789012345678".to_owned(),
            username: "fallback".to_owned(),
            global_name: Some("Longcat".to_owned()),
            avatar: Some("a_0123abcdef".to_owned()),
        })
        .expect("valid Discord user");

        assert_eq!(user.display_name, "Longcat");
        assert_eq!(
            user.avatar_url().as_deref(),
            Some(
                "https://cdn.discordapp.com/avatars/123456789012345678/a_0123abcdef.webp?size=128"
            )
        );
    }

    #[test]
    fn rejects_invalid_discord_identity_payloads() {
        for response in [
            DiscordUserResponse {
                id: "not-a-snowflake".to_owned(),
                username: "user".to_owned(),
                global_name: None,
                avatar: None,
            },
            DiscordUserResponse {
                id: "123456789012345678".to_owned(),
                username: String::new(),
                global_name: None,
                avatar: None,
            },
            DiscordUserResponse {
                id: "123456789012345678".to_owned(),
                username: "user".to_owned(),
                global_name: None,
                avatar: Some("../../avatar".to_owned()),
            },
        ] {
            assert!(matches!(
                DiscordUser::try_from(response),
                Err(DiscordClientError::InvalidUser)
            ));
        }
    }
}
