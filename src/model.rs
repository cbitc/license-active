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
    pub license_key: String,
    pub product_code: String,
    pub product_name: String,
    pub policy_name: String,
    pub issued_at: i64,
    pub entitlements: Vec<EntitlementClaim>,
    pub fingerprint_sha256: Option<String>,
    pub iat: i64,
    pub nbf: i64,
    pub exp: i64,
    pub license_expires_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementClaim {
    pub code: String,
    pub name: String,
    pub expires_at: Option<i64>,
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

/// 持久化到本地的唯一数据：token 是签名保护、自包含的权威数据源，
/// claims 等派生信息在运行时从 token 新鲜解析，不落库。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredActivation {
    pub token: String,
    pub source: ActivationSource,
    pub activated_at: i64,
}

/// 运行时状态：StoredActivation + 从 token 解析出的 claims + 当前设备指纹。
#[derive(Clone, Debug)]
pub struct Activation {
    pub token: String,
    pub source: ActivationSource,
    pub activated_at: i64,
    pub claims: TokenClaims,
    pub fingerprint: String,
}

impl Activation {
    pub fn from_verified(stored: StoredActivation, claims: TokenClaims, fingerprint: String) -> Self {
        Self {
            token: stored.token,
            source: stored.source,
            activated_at: stored.activated_at,
            claims,
            fingerprint,
        }
    }

    pub fn stored(&self) -> StoredActivation {
        StoredActivation {
            token: self.token.clone(),
            source: self.source,
            activated_at: self.activated_at,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationSource {
    Online,
    Offline,
}
