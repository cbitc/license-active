use std::{
    env,
    sync::mpsc::{self, Receiver},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use eframe::egui;

use crate::{
    api::{ApiClient, MAX_RPC_TTL},
    error::{AppError, Result},
    fingerprint,
    model::{Activation, ActivationSource},
    storage::Storage,
    token, ui,
};

#[derive(Clone)]
pub struct AppConfig {
    pub api_url: String,
    pub issuer: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let api_url =
            env::var("LICENSE_API_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());
        ApiClient::new(&api_url)?;
        Ok(Self {
            api_url,
            issuer: env::var("LICENSE_EXPECTED_ISSUER")
                .unwrap_or_else(|_| "MDT_LICENSE_SERVER".into()),
        })
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum ActivationTab {
    Online,
    Offline,
}

pub enum Notice {
    Info(String),
    Success(String),
    Error(String),
}

enum TaskResult {
    Activated(Box<Activation>),
    KeysRefreshed,
    Deactivated { license_key: String },
    Failed(String),
}

pub struct LicenseApp {
    config: Option<AppConfig>,
    storage: Option<Storage>,
    pub tab: ActivationTab,
    pub license_key: String,
    pub offline_token: String,
    pub activation: Option<Activation>,
    pub notice: Option<Notice>,
    receiver: Option<Receiver<TaskResult>>,
    busy: bool,
}

impl LicenseApp {
    pub fn new(config: AppConfig, storage: Storage) -> Self {
        let (activation, notice) = load_local_activation(&config, &storage);
        let mut app = Self {
            config: Some(config),
            storage: Some(storage),
            tab: ActivationTab::Online,
            license_key: String::new(),
            offline_token: String::new(),
            activation,
            notice,
            receiver: None,
            busy: false,
        };
        app.refresh_keys();
        app
    }

    pub fn failed(message: String) -> Self {
        Self {
            config: None,
            storage: None,
            tab: ActivationTab::Online,
            license_key: String::new(),
            offline_token: String::new(),
            activation: None,
            notice: Some(Notice::Error(message)),
            receiver: None,
            busy: false,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn activate_online(&mut self, context: egui::Context) {
        let key = self.license_key.trim().to_owned();
        if key.is_empty() {
            self.notice = Some(Notice::Error("请输入许可证密钥".into()));
            return;
        }
        let Some((config, storage)) = self.dependencies() else {
            return;
        };
        self.spawn(context, move || online_activation(&config, &storage, &key));
    }

    pub fn activate_offline(&mut self, context: egui::Context) {
        let compact = self.offline_token.trim().to_owned();
        if compact.is_empty() {
            self.notice = Some(Notice::Error("请输入离线令牌".into()));
            return;
        }
        let Some((config, storage)) = self.dependencies() else {
            return;
        };
        self.spawn(context, move || {
            offline_activation(&config, &storage, &compact)
        });
    }

    pub fn deactivate(&mut self, context: egui::Context) {
        let Some(activation) = self.activation.clone() else {
            return;
        };
        let Some((config, storage)) = self.dependencies() else {
            return;
        };
        self.busy = true;
        self.notice = Some(Notice::Info("正在取消激活...".into()));
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        thread::spawn(move || {
            let result = unbind_activation(&config, &storage, &activation);
            let _ = sender.send(result);
            context.request_repaint();
        });
    }

    fn dependencies(&mut self) -> Option<(AppConfig, Storage)> {
        match (self.config.clone(), self.storage.clone()) {
            (Some(config), Some(storage)) => Some((config, storage)),
            _ => {
                self.notice = Some(Notice::Error("应用尚未正确初始化".into()));
                None
            }
        }
    }

    fn refresh_keys(&mut self) {
        let Some((config, storage)) = self.dependencies() else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        thread::spawn(move || {
            let result = ApiClient::new(&config.api_url)
                .and_then(|api| api.signing_keys())
                .and_then(|keys| storage.save_keys(&keys))
                .map(|_| TaskResult::KeysRefreshed)
                .unwrap_or_else(|error| TaskResult::Failed(error.user_message()));
            let _ = sender.send(result);
        });
    }

    fn spawn(
        &mut self,
        context: egui::Context,
        operation: impl FnOnce() -> Result<Activation> + Send + 'static,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.notice = Some(Notice::Info("正在处理激活请求...".into()));
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        thread::spawn(move || {
            let result = operation()
                .map(|activation| TaskResult::Activated(Box::new(activation)))
                .unwrap_or_else(|error| TaskResult::Failed(error.user_message()));
            let _ = sender.send(result);
            context.request_repaint();
        });
    }

    fn poll(&mut self) {
        let result = self
            .receiver
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok());
        if let Some(result) = result {
            self.receiver = None;
            match result {
                TaskResult::Activated(activation) => {
                    self.activation = Some(*activation);
                    self.license_key.clear();
                    self.offline_token.clear();
                    self.notice = Some(Notice::Success("许可证激活成功".into()));
                    self.busy = false;
                }
                TaskResult::KeysRefreshed => {}
                TaskResult::Deactivated { license_key } => {
                    self.activation = None;
                    self.license_key = license_key;
                    self.notice = Some(Notice::Success("许可证已取消激活".into()));
                    self.busy = false;
                }
                TaskResult::Failed(message) if self.busy => {
                    self.notice = Some(Notice::Error(message));
                    self.busy = false;
                }
                TaskResult::Failed(_) => {}
            }
        }
    }
}

impl eframe::App for LicenseApp {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        if self.busy {
            context.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui::render(self, ui);
    }
}

fn online_activation(
    config: &AppConfig,
    storage: &Storage,
    license_key: &str,
) -> Result<Activation> {
    let fingerprint = fingerprint::current()?;
    let api = ApiClient::new(&config.api_url)?;
    let license = api.license(license_key)?;
    if !license.policy.require_fingerprint {
        return Err(AppError::Protocol("该许可证策略未启用设备绑定".into()));
    }
    if let Ok(keys) = api.signing_keys() {
        storage.save_keys(&keys)?;
    }
    let ttl = license.policy.max_offline_ttl_seconds.clamp(1, MAX_RPC_TTL);
    api.bind(license_key, &fingerprint)?;
    let issued = issue_and_store(config, storage, &api, license_key, &fingerprint, ttl);
    match issued {
        Ok(activation) => Ok(activation),
        Err(error) => {
            let _ = api.unbind(license_key, &fingerprint);
            Err(error)
        }
    }
}

fn issue_and_store(
    config: &AppConfig,
    storage: &Storage,
    api: &ApiClient,
    license_key: &str,
    fingerprint: &str,
    ttl: u64,
) -> Result<Activation> {
    let compact = api.issue_online(license_key, fingerprint, ttl)?;
    let claims = verify_with_refresh(config, storage, api, &compact, fingerprint)?;
    let activation = Activation {
        token: compact,
        source: ActivationSource::Online,
        activated_at: now_epoch(),
        claims,
        fingerprint: fingerprint.to_owned(),
    };
    storage.save_activation(&activation.stored())?;
    Ok(activation)
}

fn offline_activation(config: &AppConfig, storage: &Storage, compact: &str) -> Result<Activation> {
    let fingerprint = fingerprint::current()?;
    let api = ApiClient::new(&config.api_url)?;
    let claims = verify_with_refresh(config, storage, &api, compact, &fingerprint)?;
    let activation = Activation {
        token: compact.into(),
        source: ActivationSource::Offline,
        activated_at: now_epoch(),
        claims,
        fingerprint,
    };
    storage.save_activation(&activation.stored())?;
    Ok(activation)
}

fn unbind_activation(config: &AppConfig, storage: &Storage, activation: &Activation) -> TaskResult {
    let key = activation.claims.license_key.trim();
    if activation.source == ActivationSource::Offline {
        return match storage.clear_activation() {
            Ok(()) => TaskResult::Deactivated {
                license_key: key.to_owned(),
            },
            Err(error) => TaskResult::Failed(error.user_message()),
        };
    }
    let api = match ApiClient::new(&config.api_url) {
        Ok(api) => api,
        Err(error) => return TaskResult::Failed(error.user_message()),
    };
    if let Err(error) = api.unbind(key, &activation.fingerprint) {
        return TaskResult::Failed(error.user_message());
    }
    match storage.clear_activation() {
        Ok(()) => TaskResult::Deactivated {
            license_key: key.to_owned(),
        },
        Err(error) => {
            let rollback_ok = api.bind(key, &activation.fingerprint).is_ok();
            let message = if rollback_ok {
                format!(
                    "服务器已解绑，但清除本地令牌失败：{}。已恢复服务器绑定，请重试",
                    error.user_message()
                )
            } else {
                format!(
                    "服务器已解绑，但清除本地令牌失败且恢复服务器绑定失败：{}。请重试解绑",
                    error.user_message()
                )
            };
            TaskResult::Failed(message)
        }
    }
}

fn load_local_activation(
    config: &AppConfig,
    storage: &Storage,
) -> (Option<Activation>, Option<Notice>) {
    let stored = match storage.load_activation() {
        Ok(stored) => stored,
        Err(error) => {
            return (
                None,
                Some(Notice::Error(format!(
                    "读取本地激活数据失败：{}",
                    error.user_message()
                ))),
            );
        }
    };
    let Some(stored) = stored else {
        return (None, None);
    };
    let fingerprint = match fingerprint::current() {
        Ok(fingerprint) => fingerprint,
        Err(error) => {
            return (
                None,
                Some(Notice::Error(format!(
                    "无法计算设备指纹：{}。请检查系统环境后重启",
                    error.user_message()
                ))),
            );
        }
    };
    let keys = match storage.load_keys() {
        Ok(keys) => keys,
        Err(error) => {
            return (
                None,
                Some(Notice::Error(format!(
                    "读取本地签名公钥失败：{}。请检查系统环境后重启",
                    error.user_message()
                ))),
            );
        }
    };
    match token::verify(
        &stored.token,
        &keys,
        &config.issuer,
        &fingerprint,
        now_epoch(),
    ) {
        Ok(claims) => (
            Some(Activation::from_verified(stored, claims, fingerprint)),
            None,
        ),
        Err(error) => {
            let notice = match storage.clear_activation() {
                Ok(()) => Notice::Error(format!("本地环境校验失败：{}，请重新绑定", error)),
                Err(_) => Notice::Error(format!("请使用reset功能重置本地环境")),
            };
            (None, Some(notice))
        }
    }
}

fn verify_with_refresh(
    config: &AppConfig,
    storage: &Storage,
    api: &ApiClient,
    compact: &str,
    fingerprint: &str,
) -> Result<crate::model::TokenClaims> {
    let first = token::verify(
        compact,
        &storage.load_keys()?,
        &config.issuer,
        fingerprint,
        now_epoch(),
    );
    if first.is_ok() {
        return first;
    }
    if let Ok(keys) = api.signing_keys() {
        storage.save_keys(&keys)?;
        return token::verify(
            compact,
            &storage.load_keys()?,
            &config.issuer,
            fingerprint,
            now_epoch(),
        );
    }
    first
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    use super::*;
    use crate::model::{PublicJwk, StoredActivation};

    fn test_config() -> AppConfig {
        AppConfig {
            api_url: "http://127.0.0.1:3000".into(),
            issuer: "test-issuer".into(),
        }
    }

    fn signed_v3_token(claims: serde_json::Value) -> (String, PublicJwk) {
        let signing = SigningKey::from_bytes(&[9_u8; 32]);
        let header = json!({"alg":"EdDSA","typ":"license+jwt","kid":"app-test-key"});
        let h = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let p = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let input = format!("{h}.{p}");
        let signature = signing.sign(input.as_bytes());
        let token = format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()));
        let jwk = PublicJwk {
            kty: "OKP".into(),
            crv: "Ed25519".into(),
            x: URL_SAFE_NO_PAD.encode(signing.verifying_key().as_bytes()),
            kid: "app-test-key".into(),
            alg: Some("EdDSA".into()),
            key_use: Some("sig".into()),
        };
        (token, jwk)
    }

    fn base_claims(fingerprint: &str, overrides: serde_json::Value) -> serde_json::Value {
        let now = now_epoch();
        let mut claims = json!({
            "version": 3, "meta": {}, "iss": "test-issuer", "aud": "product",
            "sub": "license", "jti": "issuance", "policyId": "policy",
            "licenseKey": "LIC-TEST", "productCode": "product",
            "productName": "产品", "policyName": "策略", "issuedAt": now - 10,
            "entitlements": [{"code": "FEATURE_A", "name": "功能A"}],
            "fingerprintSha256": fingerprint,
            "iat": now - 10, "nbf": now - 10, "exp": now + 3600
        });
        for (key, value) in overrides.as_object().unwrap() {
            claims[key] = value.clone();
        }
        claims
    }

    fn store_offline_activation(storage: &Storage, token: String) {
        storage
            .save_activation(&StoredActivation {
                token,
                source: ActivationSource::Offline,
                activated_at: now_epoch(),
            })
            .unwrap();
    }

    #[test]
    fn load_accepts_valid_local_token() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().join("test.db")).unwrap();
        let fingerprint = fingerprint::current().unwrap();
        let (token, jwk) = signed_v3_token(base_claims(&fingerprint, json!({})));
        storage.save_keys(&[jwk]).unwrap();
        store_offline_activation(&storage, token);

        let (activation, notice) = load_local_activation(&test_config(), &storage);
        let activation = activation.expect("valid token must load");
        assert!(notice.is_none());
        assert_eq!(activation.claims.license_key, "LIC-TEST");
        assert_eq!(activation.claims.product_name, "产品");
        assert_eq!(activation.claims.entitlements[0].name, "功能A");
        assert_eq!(activation.fingerprint, fingerprint);
        assert_eq!(activation.source, ActivationSource::Offline);
        assert_eq!(
            storage.load_activation().unwrap().unwrap().token,
            activation.token
        );
    }

    #[test]
    fn load_without_token_starts_unbound() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().join("test.db")).unwrap();

        let (activation, notice) = load_local_activation(&test_config(), &storage);
        assert!(activation.is_none());
        assert!(notice.is_none());
    }

    #[test]
    fn load_clears_broken_local_token() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().join("test.db")).unwrap();
        let fingerprint = fingerprint::current().unwrap();
        let (mut token, jwk) = signed_v3_token(base_claims(&fingerprint, json!({})));
        token.push('x');
        storage.save_keys(&[jwk]).unwrap();
        store_offline_activation(&storage, token);

        let (activation, notice) = load_local_activation(&test_config(), &storage);
        assert!(activation.is_none());
        assert!(matches!(notice, Some(Notice::Error(_))));
        assert!(storage.load_activation().unwrap().is_none());
    }

    #[test]
    fn load_clears_expired_local_token() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().join("test.db")).unwrap();
        let fingerprint = fingerprint::current().unwrap();
        let now = now_epoch();
        let (token, jwk) = signed_v3_token(base_claims(&fingerprint, json!({"exp": now - 1})));
        storage.save_keys(&[jwk]).unwrap();
        store_offline_activation(&storage, token);

        let (activation, notice) = load_local_activation(&test_config(), &storage);
        assert!(activation.is_none());
        assert!(matches!(notice, Some(Notice::Error(_))));
        assert!(storage.load_activation().unwrap().is_none());
    }
}
