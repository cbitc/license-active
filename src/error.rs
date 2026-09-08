use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("配置错误: {0}")]
    Config(String),
    #[error("网络请求失败: {0}")]
    Network(String),
    #[error("服务端返回了无效数据: {0}")]
    Protocol(String),
    #[error("许可证令牌无效: {0}")]
    Token(String),
    #[error("本地存储错误: {0}")]
    Storage(String),
    #[error("设备指纹生成失败: {0}")]
    Fingerprint(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl AppError {
    pub fn user_message(&self) -> String {
        self.to_string()
    }

    pub fn server(code: Option<&str>, fallback: &str) -> Self {
        let message = match code {
            Some("LICENSE_NOT_FOUND") => "未找到该许可证，请检查许可证密钥",
            Some("MACHINE_LIMIT_EXCEEDED") => "该许可证的设备席位已用完",
            Some("LICENSE_MAX_MACHINES_REACHED") => "该许可证的设备席位已用完",
            Some("LICENSE_NOT_ACTIVE") => "该许可证当前不可激活",
            Some("LICENSE_INACTIVE") => "该许可证当前不可激活",
            Some("LICENSE_EXPIRED") => "该许可证已过期",
            Some("MACHINE_NOT_FOUND") => "当前设备未绑定该许可证",
            Some("MACHINE_ALREADY_REGISTERED") => "当前设备已经注册",
            _ => fallback,
        };
        Self::Protocol(message.to_owned())
    }

    pub fn server_code(message: &str, data: Option<&serde_json::Value>) -> Self {
        let code = data
            .and_then(|value| value.get("code"))
            .and_then(|value| value.as_str());
        let inferred = if message.contains("not found") {
            Some("LICENSE_NOT_FOUND")
        } else if message.contains("maximum") {
            Some("LICENSE_MAX_MACHINES_REACHED")
        } else if message.contains("expired") {
            Some("LICENSE_EXPIRED")
        } else if message.contains("inactive") {
            Some("LICENSE_INACTIVE")
        } else {
            None
        };
        Self::server(code.or(inferred), message)
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Storage(value.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::Protocol(value.to_string())
    }
}
