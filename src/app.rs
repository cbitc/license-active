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
    model::{ActivationSource, StoredActivation},
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
    Activated(Box<StoredActivation>),
    KeysRefreshed,
    Deactivated,
    Failed(String),
}

pub struct LicenseApp {
    config: Option<AppConfig>,
    storage: Option<Storage>,
    pub tab: ActivationTab,
    pub license_key: String,
    pub offline_token: String,
    pub activation: Option<StoredActivation>,
    pub notice: Option<Notice>,
    receiver: Option<Receiver<TaskResult>>,
    busy: bool,
}

impl LicenseApp {
    pub fn new(config: AppConfig, storage: Storage) -> Self {
        let activation = storage.load_activation().ok().flatten();
        let mut app = Self {
            config: Some(config),
            storage: Some(storage),
            tab: ActivationTab::Online,
            license_key: String::new(),
            offline_token: String::new(),
            activation,
            notice: None,
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
        let Some(key) = activation.license_key else {
            self.notice = Some(Notice::Error("离线激活没有可撤销的在线设备席位".into()));
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
            let result = ApiClient::new(&config.api_url)
                .and_then(|api| api.unbind(&key, &activation.fingerprint))
                .and_then(|_| storage.clear_activation())
                .map(|_| TaskResult::Deactivated)
                .unwrap_or_else(|error| TaskResult::Failed(error.user_message()));
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
        operation: impl FnOnce() -> Result<StoredActivation> + Send + 'static,
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
                TaskResult::Deactivated => {
                    self.activation = None;
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
) -> Result<StoredActivation> {
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
    let compact = match api.issue_online(license_key, &fingerprint, ttl) {
        Ok(compact) => compact,
        Err(error) => {
            let _ = api.unbind(license_key, &fingerprint);
            return Err(error);
        }
    };
    let claims = verify_with_refresh(config, storage, &api, &compact, &fingerprint)?;
    let activation = StoredActivation {
        token: compact,
        claims,
        fingerprint,
        source: ActivationSource::Online,
        license_key: Some(license_key.to_owned()),
        license: Some(license),
        activated_at: now_epoch(),
    };
    storage.save_activation(&activation)?;
    Ok(activation)
}

fn offline_activation(
    config: &AppConfig,
    storage: &Storage,
    compact: &str,
) -> Result<StoredActivation> {
    let fingerprint = fingerprint::current()?;
    let api = ApiClient::new(&config.api_url)?;
    let claims = verify_with_refresh(config, storage, &api, compact, &fingerprint)?;
    let activation = StoredActivation {
        token: compact.into(),
        claims,
        fingerprint,
        source: ActivationSource::Offline,
        license_key: None,
        license: None,
        activated_at: now_epoch(),
    };
    storage.save_activation(&activation)?;
    Ok(activation)
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
