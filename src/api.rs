use std::time::Duration;

use reqwest::{StatusCode, Url, blocking::Client};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    error::{AppError, Result},
    model::{LicenseDetails, PublicJwk},
};

pub const MAX_RPC_TTL: u64 = 12 * 30 * 24 * 60 * 60;

#[derive(Clone)]
pub struct ApiClient {
    base_url: Url,
    client: Client,
}

impl ApiClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let base_url = Url::parse(base_url)
            .map_err(|error| AppError::Config(format!("服务地址无效: {error}")))?;
        validate_transport(&base_url)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("license-active/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| AppError::Network(error.to_string()))?;
        Ok(Self { base_url, client })
    }

    pub fn license(&self, key: &str) -> Result<LicenseDetails> {
        let mut url = self.endpoint("api/v1/portal/licenses")?;
        url.path_segments_mut()
            .map_err(|_| AppError::Config("服务地址不能作为 API 基础地址".into()))?
            .push(key);
        let response = self.client.get(url).send().map_err(network_error)?;
        parse_data_response(response)
    }

    pub fn signing_keys(&self) -> Result<Vec<PublicJwk>> {
        let response = self
            .client
            .get(self.endpoint("api/v1/signing-keys")?)
            .send()
            .map_err(network_error)?;
        let body: KeysResponse = parse_json_response(response)?;
        if body.keys.is_empty() {
            return Err(AppError::Protocol("服务端没有返回签名公钥".into()));
        }
        Ok(body.keys)
    }

    pub fn issue_online(&self, key: &str, fingerprint: &str, ttl: u64) -> Result<String> {
        let body = RpcRequest {
            jsonrpc: "2.0",
            method: "issueMachineBindingOnline",
            params: RpcParams {
                license_key: key,
                fingerprint,
                ttl: ttl.min(MAX_RPC_TTL),
            },
        };
        let response = self
            .client
            .post(self.endpoint("api/v1/rpc")?)
            .json(&body)
            .send()
            .map_err(network_error)?;
        let status = response.status();
        let parsed: RpcResponse = response
            .json()
            .map_err(|_| AppError::Protocol("无法解析激活服务响应".into()))?;
        if let Some(error) = parsed.error {
            let code = error
                .data
                .as_ref()
                .and_then(|data| data.get("code"))
                .and_then(|value| value.as_str());
            return Err(AppError::server(code, &error.message));
        }
        if !status.is_success() {
            return Err(AppError::Protocol(format!("激活服务返回 HTTP {status}")));
        }
        parsed
            .result
            .ok_or_else(|| AppError::Protocol("激活服务未返回许可证令牌".into()))
    }

    fn endpoint(&self, path: &str) -> Result<Url> {
        let mut base = self.base_url.clone();
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        base.join(path)
            .map_err(|error| AppError::Config(error.to_string()))
    }
}

fn validate_transport(url: &Url) -> Result<()> {
    if url.scheme() == "https" {
        return Ok(());
    }
    let local = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if url.scheme() == "http" && local {
        return Ok(());
    }
    Err(AppError::Config("远程许可证服务必须使用 HTTPS".into()))
}

fn network_error(error: reqwest::Error) -> AppError {
    let message = if error.is_timeout() {
        "连接许可证服务超时".into()
    } else {
        error.to_string()
    };
    AppError::Network(message)
}

fn parse_data_response<T: DeserializeOwned>(response: reqwest::blocking::Response) -> Result<T> {
    let status = response.status();
    if status.is_success() {
        return response
            .json::<DataResponse<T>>()
            .map(|body| body.data)
            .map_err(|_| AppError::Protocol("无法解析许可证服务响应".into()));
    }
    parse_http_error(response, status)
}

fn parse_json_response<T: DeserializeOwned>(response: reqwest::blocking::Response) -> Result<T> {
    let status = response.status();
    if status.is_success() {
        response
            .json()
            .map_err(|_| AppError::Protocol("无法解析许可证服务响应".into()))
    } else {
        parse_http_error(response, status)
    }
}

fn parse_http_error<T>(response: reqwest::blocking::Response, status: StatusCode) -> Result<T> {
    let body = response.json::<HttpErrorResponse>().ok();
    let code = body.as_ref().map(|body| body.error.code.as_str());
    let fallback = body
        .as_ref()
        .map(|body| body.error.message.as_str())
        .unwrap_or("许可证服务请求失败");
    Err(AppError::server(
        code,
        &format!("{fallback} (HTTP {status})"),
    ))
}

#[derive(Deserialize)]
struct DataResponse<T> {
    data: T,
}

#[derive(Deserialize)]
struct KeysResponse {
    keys: Vec<PublicJwk>,
}

#[derive(Deserialize)]
struct HttpErrorResponse {
    error: HttpError,
}

#[derive(Deserialize)]
struct HttpError {
    code: String,
    message: String,
}

#[derive(Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'static str,
    method: &'static str,
    params: RpcParams<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RpcParams<'a> {
    license_key: &'a str,
    fingerprint: &'a str,
    ttl: u64,
}

#[derive(Deserialize)]
struct RpcResponse {
    result: Option<String>,
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    message: String,
    data: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_insecure_remote_urls() {
        assert!(ApiClient::new("http://example.com").is_err());
        assert!(ApiClient::new("http://localhost:3000").is_ok());
        assert!(ApiClient::new("https://example.com").is_ok());
    }

    #[test]
    fn ttl_limit_matches_server_contract() {
        assert_eq!(MAX_RPC_TTL, 31_104_000);
    }
}
