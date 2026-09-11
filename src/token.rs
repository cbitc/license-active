use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;

use crate::{
    error::{AppError, Result},
    model::{PublicJwk, TokenClaims},
};

#[derive(Deserialize)]
struct Header {
    alg: String,
    typ: Option<String>,
    kid: String,
}

pub fn verify(
    compact: &str,
    keys: &[PublicJwk],
    fingerprint: &str,
    now: i64,
) -> Result<TokenClaims> {
    let parts: Vec<&str> = compact.trim().split('.').collect();
    if parts.len() != 3 {
        return Err(AppError::Token("令牌格式不正确".into()));
    }
    let header: Header = decode_json(parts[0])?;
    if header.alg != "EdDSA" || header.typ.as_deref() != Some("license+jwt") {
        return Err(AppError::Token("令牌算法或类型不受支持".into()));
    }
    let jwk = keys
        .iter()
        .find(|key| key.kid == header.kid)
        .ok_or_else(|| AppError::Token("找不到令牌对应的签名公钥".into()))?;
    if jwk.kty != "OKP" || jwk.crv != "Ed25519" {
        return Err(AppError::Token("签名公钥类型不受支持".into()));
    }
    let key_bytes = URL_SAFE_NO_PAD
        .decode(&jwk.x)
        .map_err(|_| AppError::Token("签名公钥编码无效".into()))?;
    let key_array: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| AppError::Token("签名公钥长度无效".into()))?;
    let verifying_key =
        VerifyingKey::from_bytes(&key_array).map_err(|_| AppError::Token("签名公钥无效".into()))?;
    let signature_bytes = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| AppError::Token("令牌签名编码无效".into()))?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| AppError::Token("令牌签名长度无效".into()))?;
    verifying_key
        .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
        .map_err(|_| AppError::Token("签名验证失败".into()))?;

    let claims: TokenClaims = decode_json(parts[1])?;
    validate_claims(&claims, fingerprint, now)?;
    Ok(claims)
}

fn decode_json<T: serde::de::DeserializeOwned>(encoded: &str) -> Result<T> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| AppError::Token("令牌内容编码无效".into()))?;
    serde_json::from_slice(&bytes).map_err(|_| AppError::Token("令牌内容无效".into()))
}

fn validate_claims(claims: &TokenClaims, fingerprint: &str, now: i64) -> Result<()> {
    if claims.version != 3 {
        return Err(AppError::Token("令牌版本不受支持".into()));
    }
    if claims.sub.trim().is_empty()
        || claims.aud.trim().is_empty()
        || claims.jti.trim().is_empty()
        || claims.policy_id.trim().is_empty()
        || claims.license_key.trim().is_empty()
    {
        return Err(AppError::Token("令牌缺少必要声明".into()));
    }
    if claims
        .entitlements
        .iter()
        .any(|item| item.code.trim().is_empty())
    {
        return Err(AppError::Token("令牌授权项无效".into()));
    }
    if now < claims.nbf {
        return Err(AppError::Token("令牌尚未生效".into()));
    }
    if now >= claims.exp {
        return Err(AppError::Token("令牌已过期".into()));
    }
    if claims.iat > claims.exp || claims.nbf > claims.exp {
        return Err(AppError::Token("令牌时间范围无效".into()));
    }
    if let Some(bound) = &claims.fingerprint_sha256
        && bound != fingerprint
    {
        return Err(AppError::Token("令牌不属于当前设备".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    use super::*;

    fn signed_token(overrides: serde_json::Value) -> (String, PublicJwk) {
        let signing = SigningKey::from_bytes(&[7_u8; 32]);
        let header = json!({"alg":"EdDSA","typ":"license+jwt","kid":"test-key"});
        let mut claims = json!({
            "version": 3, "meta": {}, "iss": "issuer", "aud": "product",
            "sub": "license", "jti": "issuance", "policyId": "policy",
            "licenseKey": "LIC-TEST", "productCode": "product",
            "productName": "产品", "policyName": "策略", "issuedAt": 100,
            "entitlements": [{"code": "FEATURE_A", "name": "功能A"}],
            "fingerprintSha256": "fingerprint",
            "iat": 100, "nbf": 100, "exp": 200
        });
        for (key, value) in overrides.as_object().unwrap() {
            claims[key] = value.clone();
        }
        let h = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let p = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let input = format!("{h}.{p}");
        let signature = signing.sign(input.as_bytes());
        let token = format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()));
        let jwk = PublicJwk {
            kty: "OKP".into(),
            crv: "Ed25519".into(),
            x: URL_SAFE_NO_PAD.encode(signing.verifying_key().as_bytes()),
            kid: "test-key".into(),
            alg: Some("EdDSA".into()),
            key_use: Some("sig".into()),
        };
        (token, jwk)
    }

    #[test]
    fn verifies_valid_token() {
        let (token, key) = signed_token(json!({}));
        assert!(verify(&token, &[key], "fingerprint", 150).is_ok());
    }

    #[test]
    fn rejects_invalid_claims_and_signature() {
        for change in [
            json!({"version": 1}),
            json!({"version": 2}),
            json!({"nbf": 160}),
            json!({"exp": 150}),
            json!({"fingerprintSha256": "other"}),
            json!({"entitlements": [{"code": "  ", "name": "功能A"}]}),
            json!({"licenseKey": "   "}),
        ] {
            let (token, key) = signed_token(change);
            assert!(verify(&token, &[key], "fingerprint", 150).is_err());
        }
        let (mut token, key) = signed_token(json!({}));
        token.push('x');
        assert!(verify(&token, &[key], "fingerprint", 150).is_err());
    }

    #[test]
    fn rejects_unknown_key() {
        let (token, _) = signed_token(json!({}));
        assert!(verify(&token, &[], "fingerprint", 150).is_err());
    }
}
