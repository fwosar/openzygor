use crate::model::{Access, Account, Game, Package, Region, Session};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use reqwest::{Client, Response, Url, header};
use serde::Deserialize;
use std::{collections::BTreeMap, time::Duration};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

pub const ENDPOINT: &str = "https://data.zygorguides.com/updater/packager.php";
pub const PROTOCOL_CLIENT_VERSION: &str = "4.13.2";
const MAX_JSON: usize = 8 * 1024 * 1024;
const MAX_ARCHIVE: usize = 256 * 1024 * 1024;

#[derive(Clone)]
pub struct Api {
    client: Client,
    endpoint: Url,
}

#[derive(Deserialize)]
struct User {
    #[serde(default)]
    login: String,
    #[serde(default, alias = "displayName")]
    displayname: Option<String>,
    #[serde(default)]
    token: String,
}
#[derive(Deserialize)]
struct Validation {
    #[serde(default)]
    success: Option<bool>,
    user: User,
    #[serde(default)]
    access: Access,
}
#[derive(Deserialize)]
pub struct Catalogue {
    pub success: bool,
    pub packages: Vec<Package>,
    #[serde(default)]
    pub access: Option<Access>,
}

impl Api {
    pub fn production() -> Result<Self> {
        Self::new(ENDPOINT)
    }
    /// Only HTTPS Zygor and loopback test servers are accepted.
    pub fn new(endpoint: &str) -> Result<Self> {
        let endpoint = Url::parse(endpoint)?;
        let host = endpoint.host_str().unwrap_or_default();
        ensure!(
            (endpoint.scheme() == "https" && host == "data.zygorguides.com")
                || (endpoint.scheme() == "http"
                    && matches!(host, "127.0.0.1" | "localhost" | "[::1]")),
            "Unsupported service address"
        );
        let mut headers = header::HeaderMap::new();
        headers.insert("X-Zygor-Api", header::HeaderValue::from_static("4"));
        headers.insert(
            "X-Zygor-UV",
            header::HeaderValue::from_static(PROTOCOL_CLIENT_VERSION),
        );
        headers.insert("X-Zygor-OS", header::HeaderValue::from_static("linux"));
        headers.insert(
            "X-Zygor-ServerRestart",
            header::HeaderValue::from_static("false"),
        );
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static("application/json"),
        );
        let client = Client::builder()
            .default_headers(headers)
            .user_agent(concat!("OpenZygor/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { client, endpoint })
    }

    fn params(&self, action: &str) -> BTreeMap<String, String> {
        [
            ("do", action),
            ("uv", PROTOCOL_CLIENT_VERSION),
            ("full", "1"),
            ("wwwdev", "0"),
            ("source", "OpenZygor"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        params: &BTreeMap<String, String>,
    ) -> Result<T> {
        let response = self
            .client
            .get(self.endpoint.clone())
            .query(params)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(network_error)?;
        let bytes = read_response(response, MAX_JSON, &CancellationToken::new(), |_, _| {}).await?;
        // serde errors can quote the offending value, including account secrets.
        serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Zygor returned an unexpected response format"))
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<Session> {
        ensure!(
            !username.trim().is_empty() && !password.is_empty(),
            "Enter your Zygor username and password"
        );
        ensure!(
            !username.contains('@'),
            "Use your Zygor username, rather than your email address"
        );
        let encoded = Zeroizing::new(format!("Qö{}", STANDARD.encode(password.as_bytes())));
        let mut params = self.params("validate");
        params.insert("user".into(), username.trim().into());
        params.insert("pass".into(), encoded.to_string());
        params.insert("auto".into(), "false".into());
        let result = self.get_json::<Validation>(&params).await;
        if let Some(mut pass) = params.remove("pass") {
            zeroize::Zeroize::zeroize(&mut pass);
        }
        session(result?)
    }

    pub async fn validate(&self, token: &str) -> Result<Session> {
        let mut params = self.params("validate");
        params.insert("token".into(), token.into());
        params.insert("auto".into(), "true".into());
        session(self.get_json(&params).await?)
    }

    pub async fn catalogue(
        &self,
        token: &str,
        region: Region,
        installed: &BTreeMap<String, Option<String>>,
    ) -> Result<Catalogue> {
        let mut params = self.params("list_packages");
        params.insert("token".into(), token.into());
        params.insert("region".into(), region.id().into());
        params.insert("installed".into(), serde_json::to_string(installed)?);
        let result: Catalogue = self.get_json(&params).await?;
        ensure!(
            result.success,
            "Zygor could not list the packages for this account"
        );
        Ok(result)
    }

    pub async fn download(
        &self,
        request: DownloadRequest<'_>,
        cancel: &CancellationToken,
        progress: impl Fn(u64, Option<u64>),
    ) -> Result<Vec<u8>> {
        let mut params = self.params("comp");
        params.insert("token".into(), request.token.into());
        params.insert("package".into(), request.game.package().into());
        params.insert("region".into(), request.region.id().into());
        params.insert("from".into(), request.from.unwrap_or("0.0.000").into());
        let sending = self
            .client
            .post(self.endpoint.clone())
            .query(&params)
            .form(&[("list", request.manifest)])
            .send();
        let response = tokio::select! {
            _ = cancel.cancelled() => bail!("Download cancelled"),
            response = sending => response.map_err(network_error)?,
        };
        let data = read_response(response, MAX_ARCHIVE, cancel, progress).await?;
        ensure!(
            data.starts_with(b"PK\x03\x04") || data.starts_with(b"PK\x05\x06"),
            "Zygor did not return a guide archive; your account or subscription may need attention"
        );
        Ok(data)
    }

    pub async fn license(&self, token: &str, game: Game) -> Result<String> {
        let mut params = self.params("license");
        params.insert("token".into(), token.into());
        params.insert("package".into(), game.package().into());
        let response = self
            .client
            .get(self.endpoint.clone())
            .query(&params)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(network_error)?;
        let bytes = read_response(response, MAX_JSON, &CancellationToken::new(), |_, _| {}).await?;
        let text = String::from_utf8(bytes).context("Zygor returned an invalid licence")?;
        ensure!(
            !text.trim().is_empty() && !text.trim_start().starts_with(['{', '<']),
            "Zygor did not return a guide licence"
        );
        Ok(text)
    }
}

pub struct DownloadRequest<'a> {
    pub token: &'a str,
    pub game: Game,
    pub region: Region,
    pub from: Option<&'a str>,
    pub manifest: &'a str,
}

fn session(response: Validation) -> Result<Session> {
    ensure!(
        response.success != Some(false) && !response.user.token.is_empty(),
        "Zygor did not accept this login"
    );
    let user = response.user;
    Ok(Session {
        token: Zeroizing::new(user.token),
        account: Account {
            display_name: user
                .displayname
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| user.login.clone()),
            username: user.login,
            access: response.access,
        },
    })
}

fn network_error(error: reqwest::Error) -> anyhow::Error {
    // reqwest errors contain the request URL, which carries credentials in this protocol.
    if error.is_timeout() {
        anyhow::anyhow!("The connection to Zygor timed out. Try again.")
    } else {
        anyhow::anyhow!("Could not connect to Zygor. Check your internet connection and try again.")
    }
}

async fn read_response(
    response: Response,
    limit: usize,
    cancel: &CancellationToken,
    progress: impl Fn(u64, Option<u64>),
) -> Result<Vec<u8>> {
    match response.status().as_u16() {
        200..=299 => (),
        401 => bail!(
            "Your Zygor login was rejected or expired. Sign in again; contact Zygor support if your account is locked."
        ),
        403 => bail!("Your account does not have access to this download"),
        429 => bail!("Zygor is rate limiting requests. Wait a moment before trying again."),
        503 => bail!("Zygor is undergoing maintenance. Try again later."),
        status => bail!("Zygor returned HTTP {status}. Try again later."),
    }
    let total = response.content_length();
    ensure!(
        total.unwrap_or(0) <= limit as u64,
        "The server response exceeds the size limit"
    );
    let mut stream = response.bytes_stream();
    let mut data = Vec::new();
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => bail!("Download cancelled"),
            value = tokio::time::timeout(Duration::from_secs(30), stream.next()) => value.context("The download stalled. Try again.")?,
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk.map_err(network_error)?;
        ensure!(
            data.len() + chunk.len() <= limit,
            "The server response exceeds the size limit"
        );
        data.extend_from_slice(&chunk);
        progress(data.len() as u64, total);
    }
    Ok(data)
}
