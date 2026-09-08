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
            Some("LICENSE_NOT_ACTIVE") => "该许可证当前不可激活",
            Some("MACHINE_ALREADY_REGISTERED") => "当前设备已经注册",
            _ => fallback,
        };
        Self::Protocol(message.to_owned())
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
