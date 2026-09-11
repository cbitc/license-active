mod api;
mod app;
mod error;
mod fingerprint;
mod model;
mod storage;
mod token;
mod ui;

use std::path::PathBuf;

use app::{AppConfig, LicenseApp};
use directories::ProjectDirs;
use eframe::egui;
use error::{AppError, Result};
use storage::Storage;

fn main() -> eframe::Result<()> {
    dotenv::dotenv().ok();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 680.0])
            .with_min_inner_size([620.0, 520.0])
            .with_title("许可证激活"),
        ..Default::default()
    };

    eframe::run_native(
        "许可证激活",
        options,
        Box::new(|cc| {
            ui::configure_fonts(&cc.egui_ctx);
            Ok(Box::new(create_app()))
        }),
    )
}

fn create_app() -> LicenseApp {
    match initialize() {
        Ok((config, storage)) => LicenseApp::new(config, storage),
        Err(error) => LicenseApp::failed(error.user_message()),
    }
}

fn initialize() -> Result<(AppConfig, Storage)> {
    let config = AppConfig::from_env()?;
    let project_dirs = ProjectDirs::from("com", "license-tools", "license-active")
        .ok_or_else(|| AppError::Config("无法确定应用数据目录".into()))?;
    let data_dir: PathBuf = project_dirs.data_local_dir().into();
    std::fs::create_dir_all(&data_dir)?;
    let storage = Storage::open(data_dir.join("license.db"))?;
    storage.seed_builtin_key()?;
    Ok((config, storage))
}
