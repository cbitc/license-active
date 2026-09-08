use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseDetails {
    pub id: String,
    pub name: Option<String>,
    pub status: String,
    pub effective_status: String,
    pub expires_at: Option<String>,
    pub product: Product,
    pub policy: Policy,
    pub entitlements: Vec<Entitlement>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Product {
    pub id: String,
    pub code: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub id: String,
    pub name: String,
    pub require_fingerprint: bool,
    pub max_machines: Option<u32>,
    pub max_offline_ttl_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entitlement {
    pub code: String,
    pub name: String,
    pub expires_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenClaims {
    pub version: u8,
    #[serde(default)]
    pub meta: serde_json::Value,
    pub iss: String,
    pub aud: String,
    pub sub: String,
    pub jti: String,
    pub policy_id: String,
    pub user_id: Option<String>,
    pub entitlements: Vec<String>,
    pub fingerprint_sha256: Option<String>,
    pub iat: i64,
    pub nbf: i64,
    pub exp: i64,
    pub license_expires_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PublicJwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub kid: String,
    pub alg: Option<String>,
    #[serde(rename = "use")]
    pub key_use: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredActivation {
    pub token: String,
    pub claims: TokenClaims,
    pub fingerprint: String,
    pub source: ActivationSource,
    pub license: Option<LicenseDetails>,
    pub activated_at: i64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationSource {
    Online,
    Offline,
}
