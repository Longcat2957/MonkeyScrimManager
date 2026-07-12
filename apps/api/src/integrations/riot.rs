use futures_util::future::BoxFuture;
use reqwest::{Client, StatusCode, Url};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

const RIOT_RECORDS_PATH: &str = "records";
const RIOT_RECORDS_REFRESH_PATH: &str = "records/refresh";
const RIOT_ACCOUNT_REFRESH_PATH: &str = "riot-accounts/refresh";
const CONNECT_TIMEOUT_SECONDS: u64 = 2;
const REQUEST_TIMEOUT_SECONDS: u64 = 4;
const USER_AGENT: &str = "MonkeyBot/0.1 (+https://monkeybot.lol)";

pub trait RiotClient: Send + Sync {
    fn records<'a>(
        &'a self,
        query: &'a str,
        region: &'a str,
    ) -> BoxFuture<'a, Result<RiotRecordsPayload, RiotClientError>>;

    fn refresh_records<'a>(
        &'a self,
        query: &'a str,
        region: &'a str,
    ) -> BoxFuture<'a, Result<RiotRecordsPayload, RiotClientError>>;

    fn refresh_riot_account<'a>(
        &'a self,
        puuid: &'a str,
        platform: &'a str,
        routing_region: &'a str,
    ) -> BoxFuture<'a, Result<RiotRecordsPayload, RiotClientError>>;
}

#[derive(Clone)]
pub struct ReqwestRiotClient {
    http: Client,
    base_url: Url,
    token: Option<SecretString>,
}

impl ReqwestRiotClient {
    pub fn new(base_url: Url, token: Option<SecretString>) -> Self {
        let http = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECONDS))
            .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECONDS))
            .user_agent(USER_AGENT)
            .build()
            .expect("Riot client HTTP initialization should not fail");
        Self {
            http,
            base_url,
            token,
        }
    }

    fn with_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.token {
            request.bearer_auth(token.expose_secret())
        } else {
            request
        }
    }

    async fn records_inner(
        &self,
        path: &str,
        query: &str,
        region: &str,
    ) -> Result<RiotRecordsPayload, RiotClientError> {
        if query.trim().is_empty() || region.trim().is_empty() {
            return Err(RiotClientError::InvalidRequest);
        }
        let mut endpoint = self.endpoint(path)?;
        endpoint
            .query_pairs_mut()
            .clear()
            .extend_pairs([("query", query.trim()), ("region", region.trim())]);
        let response = self
            .with_auth(self.http.get(endpoint))
            .send()
            .await
            .map_err(RiotClientError::Request)?;
        ensure_success(path, response.status())?;
        let payload = response
            .json::<RiotRecordsPayload>()
            .await
            .map_err(|_| RiotClientError::MalformedPayload)?;
        Ok(payload)
    }

    async fn refresh_riot_account_inner(
        &self,
        puuid: &str,
        platform: &str,
        routing_region: &str,
    ) -> Result<RiotRecordsPayload, RiotClientError> {
        if puuid.trim().is_empty() || platform.trim().is_empty() || routing_region.trim().is_empty()
        {
            return Err(RiotClientError::InvalidRequest);
        }
        let response = self
            .with_auth(
                self.http
                    .post(self.endpoint(RIOT_ACCOUNT_REFRESH_PATH)?)
                    .json(&RefreshRiotAccountPayload {
                        puuid: puuid.to_owned(),
                        platform: platform.to_owned(),
                        routing_region: routing_region.to_owned(),
                    }),
            )
            .send()
            .await
            .map_err(RiotClientError::Request)?;
        ensure_success(RIOT_ACCOUNT_REFRESH_PATH, response.status())?;
        let payload = response
            .json::<RiotRecordsPayload>()
            .await
            .map_err(|_| RiotClientError::MalformedPayload)?;
        Ok(payload)
    }

    fn endpoint(&self, path: &str) -> Result<Url, RiotClientError> {
        self.base_url
            .join(path)
            .map_err(|_| RiotClientError::InvalidUrl)
    }
}

impl RiotClient for ReqwestRiotClient {
    fn records<'a>(
        &'a self,
        query: &'a str,
        region: &'a str,
    ) -> BoxFuture<'a, Result<RiotRecordsPayload, RiotClientError>> {
        Box::pin(self.records_inner(RIOT_RECORDS_PATH, query, region))
    }

    fn refresh_records<'a>(
        &'a self,
        query: &'a str,
        region: &'a str,
    ) -> BoxFuture<'a, Result<RiotRecordsPayload, RiotClientError>> {
        Box::pin(self.records_inner(RIOT_RECORDS_REFRESH_PATH, query, region))
    }

    fn refresh_riot_account<'a>(
        &'a self,
        puuid: &'a str,
        platform: &'a str,
        routing_region: &'a str,
    ) -> BoxFuture<'a, Result<RiotRecordsPayload, RiotClientError>> {
        Box::pin(self.refresh_riot_account_inner(puuid, platform, routing_region))
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct RiotRecordsPayload(pub Value);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RefreshRiotAccountPayload {
    puuid: String,
    platform: String,
    routing_region: String,
}

#[derive(Debug, Error)]
pub enum RiotClientError {
    #[error("Riot service URL is invalid")]
    InvalidUrl,
    #[error("Riot request parameter is invalid")]
    InvalidRequest,
    #[error("Riot request failed")]
    Request(#[source] reqwest::Error),
    #[error("Riot service rejected the request with HTTP {status} for {endpoint}")]
    Rejected { endpoint: String, status: u16 },
    #[error("Riot service returned a malformed payload")]
    MalformedPayload,
}

fn ensure_success(endpoint: &str, status: StatusCode) -> Result<(), RiotClientError> {
    if status.is_success() {
        Ok(())
    } else {
        Err(RiotClientError::Rejected {
            endpoint: endpoint.to_owned(),
            status: status.as_u16(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn riot_payload_is_json_transparent() {
        let payload = RiotRecordsPayload(Value::Object(Default::default()));
        let encoded = serde_json::to_string(&payload).expect("payload");
        assert_eq!(encoded, "{}");
    }
}
