use std::{fs, path::Path};

use chrono::{DateTime, Local};
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, RichText};

use crate::{
    app::{ActivationTab, LicenseApp, Notice},
    model::{ActivationSource, StoredActivation},
};

pub fn configure_fonts(context: &egui::Context) {
    let candidates = if cfg!(windows) {
        vec![r"C:\Windows\Fonts\msyh.ttc", r"C:\Windows\Fonts\simhei.ttf"]
    } else if cfg!(target_os = "macos") {
        vec!["/System/Library/Fonts/PingFang.ttc"]
    } else {
        vec!["/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"]
    };
    if let Some((path, bytes)) = candidates
        .into_iter()
        .find_map(|path| fs::read(path).ok().map(|bytes| (path, bytes)))
    {
        let mut fonts = FontDefinitions::default();
        fonts
            .font_data
            .insert("cjk".into(), FontData::from_owned(bytes).into());
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, "cjk".into());
        }
        context.set_fonts(fonts);
        let _ = Path::new(path);
    }
}

pub fn render(app: &mut LicenseApp, context: &egui::Context) {
    egui::CentralPanel::default().show(context, |ui| {
        ui.set_max_width(720.0);
        ui.add_space(14.0);
        ui.heading(RichText::new("许可证激活").size(26.0).strong());
        ui.label(RichText::new("管理当前设备的软件许可证").color(Color32::from_gray(115)));
        ui.add_space(18.0);

        ui.horizontal(|ui| {
            ui.selectable_value(&mut app.tab, ActivationTab::Online, "在线激活");
            ui.selectable_value(&mut app.tab, ActivationTab::Offline, "离线激活");
        });
        ui.separator();
        ui.add_space(12.0);

        match app.tab {
            ActivationTab::Online => online_form(app, ui, context),
            ActivationTab::Offline => offline_form(app, ui, context),
        }
        ui.add_space(12.0);
        notice(app, ui);
        ui.add_space(14.0);
        if let Some(activation) = &app.activation {
            activation_details(activation, ui);
        }
    });
}

fn online_form(app: &mut LicenseApp, ui: &mut egui::Ui, context: &egui::Context) {
    ui.label(RichText::new("许可证密钥").strong());
    let response = ui.add_enabled(
        !app.is_busy(),
        egui::TextEdit::singleline(&mut app.license_key)
            .password(true)
            .hint_text("请输入 license key")
            .desired_width(f32::INFINITY),
    );
    ui.add_space(8.0);
    let activate = ui.add_enabled(
        !app.is_busy(),
        egui::Button::new("激活许可证").min_size([120.0, 36.0].into()),
    );
    if activate.clicked()
        || (response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
    {
        app.activate_online(context.clone());
    }
}

fn offline_form(app: &mut LicenseApp, ui: &mut egui::Ui, context: &egui::Context) {
    ui.label(RichText::new("离线令牌").strong());
    ui.add_enabled(
        !app.is_busy(),
        egui::TextEdit::multiline(&mut app.offline_token)
            .hint_text("粘贴由许可证服务签发的离线令牌")
            .desired_rows(6)
            .desired_width(f32::INFINITY),
    );
    ui.add_space(8.0);
    if ui
        .add_enabled(
            !app.is_busy(),
            egui::Button::new("激活离线许可证").min_size([140.0, 36.0].into()),
        )
        .clicked()
    {
        app.activate_offline(context.clone());
    }
}

fn notice(app: &LicenseApp, ui: &mut egui::Ui) {
    if let Some(notice) = &app.notice {
        let (text, color) = match notice {
            Notice::Info(text) => (text, Color32::from_rgb(45, 105, 170)),
            Notice::Success(text) => (text, Color32::from_rgb(25, 125, 75)),
            Notice::Error(text) => (text, Color32::from_rgb(190, 55, 55)),
        };
        ui.label(RichText::new(text).color(color));
    }
}

fn activation_details(activation: &StoredActivation, ui: &mut egui::Ui) {
    ui.separator();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.heading(RichText::new("当前许可证").size(19.0));
        ui.add_space(8.0);
        let source = match activation.source {
            ActivationSource::Online => "在线激活",
            ActivationSource::Offline => "离线激活",
        };
        ui.label(RichText::new(source).color(Color32::from_rgb(25, 125, 75)));
    });
    ui.add_space(8.0);

    egui::Grid::new("license_details")
        .num_columns(2)
        .spacing([24.0, 9.0])
        .show(ui, |ui| {
            if let Some(license) = &activation.license {
                row(
                    ui,
                    "产品",
                    &format!("{} ({})", license.product.name, license.product.code),
                );
                row(ui, "许可证状态", &license.effective_status);
                row(ui, "策略", &license.policy.name);
            } else {
                row(ui, "产品 ID", &activation.claims.aud);
            }
            row(ui, "许可证 ID", &activation.claims.sub);
            row(ui, "令牌有效期", &format_time(activation.claims.exp));
            row(ui, "激活时间", &format_time(activation.activated_at));
        });

    ui.add_space(10.0);
    ui.label(RichText::new("授权功能").strong());
    if activation.claims.entitlements.is_empty() {
        ui.label(RichText::new("无").color(Color32::from_gray(120)));
    } else {
        ui.horizontal_wrapped(|ui| {
            for entitlement in &activation.claims.entitlements {
                ui.label(
                    RichText::new(entitlement).background_color(Color32::from_rgb(232, 238, 242)),
                );
            }
        });
    }
    ui.add_space(14.0);
    let button = ui.add_enabled(
        false,
        egui::Button::new("取消激活").min_size([100.0, 34.0].into()),
    );
    button.on_disabled_hover_text("当前服务端尚未提供安全的客户端取消激活接口");
}

fn row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(RichText::new(label).color(Color32::from_gray(105)));
    ui.label(value);
    ui.end_row();
}

fn format_time(epoch: i64) -> String {
    DateTime::from_timestamp(epoch, 0)
        .map(|time| {
            time.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "未知".into())
}
