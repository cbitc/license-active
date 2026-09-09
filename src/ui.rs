use std::fs;

use chrono::{DateTime, Local};
use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use egui_components::theme::Theme;
use egui_components::{
    Alert, Badge, Button, Card, DescriptionList, Form, Heading, Input, Label, Tabs, Tag, Variant,
};

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

    if let Some(bytes) = candidates.into_iter().find_map(|path| fs::read(path).ok()) {
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
    }

    Theme::light().install(context);
    context.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.window_margin = egui::Margin::same(18);
        for font_id in style.text_styles.values_mut() {
            font_id.size = font_id.size.max(14.0);
        }
    });
}

pub fn render(app: &mut LicenseApp, ui: &mut egui::Ui) {
    egui::Frame::central_panel(ui.style()).show(ui, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.set_max_width(720.0);
                    ui.add(
                        Heading::new("许可证激活")
                            .h1()
                            .description("管理当前设备的软件许可证"),
                    );

                    let mut tab_index = match app.tab {
                        ActivationTab::Online => 0,
                        ActivationTab::Offline => 1,
                    };
                    ui.add_space(20.0);
                    Tabs::new(&mut tab_index)
                        .segmented()
                        .tab("在线激活")
                        .tab("离线激活")
                        .show(ui);
                    app.tab = if tab_index == 0 {
                        ActivationTab::Online
                    } else {
                        ActivationTab::Offline
                    };

                    ui.add_space(12.0);
                    let context = ui.ctx().clone();
                    match app.tab {
                        ActivationTab::Online => online_form(app, ui, context),
                        ActivationTab::Offline => offline_form(app, ui, context),
                    }

                    if app.notice.is_some() {
                        ui.add_space(12.0);
                        notice(app, ui);
                    }

                    if let Some(activation) = app.activation.clone() {
                        ui.add_space(14.0);
                        activation_details(&activation, ui, app);
                    }
                });
            });
    });
}

fn online_form(app: &mut LicenseApp, ui: &mut egui::Ui, context: egui::Context) {
    let busy = app.is_busy();
    let mut submit_from_enter = false;

    Card::new()
        .title("在线激活")
        .description("连接许可证服务，为当前设备注册一个机器席位")
        .divider()
        .outline()
        .show(ui, |ui| {
            Form::new().show(ui, |form| {
                form.required("许可证密钥", |ui| {
                    let response = ui.add(
                        Input::new(&mut app.license_key)
                            .password(true)
                            .placeholder("请输入 license key")
                            .width(ui.available_width())
                            .disabled(busy),
                    );
                    submit_from_enter = response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                });
            });

            let response = ui.add(Button::primary("激活许可证").full_width().disabled(busy));
            if !busy && (response.clicked() || submit_from_enter) {
                app.activate_online(context.clone());
            }
        });
}

fn offline_form(app: &mut LicenseApp, ui: &mut egui::Ui, context: egui::Context) {
    let busy = app.is_busy();
    let mut submit = false;

    Card::new()
        .title("离线激活")
        .description("粘贴已签发的离线令牌，在无网络环境中验证当前设备")
        .divider()
        .outline()
        .show(ui, |ui| {
            Form::new().show(ui, |form| {
                form.required("离线令牌", |ui| {
                    ui.add_enabled(
                        !busy,
                        egui::TextEdit::multiline(&mut app.offline_token)
                            .hint_text("粘贴由许可证服务签发的离线令牌")
                            .desired_rows(7)
                            .desired_width(ui.available_width()),
                    );
                });
            });

            if ui
                .add(
                    Button::primary("激活离线许可证")
                        .full_width()
                        .disabled(busy),
                )
                .clicked()
            {
                submit = true;
            }
            if !busy && submit {
                app.activate_offline(context.clone());
            }
        });
}

fn notice(app: &LicenseApp, ui: &mut egui::Ui) {
    if let Some(notice) = &app.notice {
        let alert = match notice {
            Notice::Info(text) => Alert::new(text.clone()).title("处理中").info(),
            Notice::Success(text) => Alert::new(text.clone()).title("完成").success(),
            Notice::Error(text) => Alert::new(text.clone()).title("操作失败").danger(),
        };
        ui.add(alert);
    }
}

fn activation_details(activation: &StoredActivation, ui: &mut egui::Ui, app: &mut LicenseApp) {
    let source = match activation.source {
        ActivationSource::Online => ("在线激活", Variant::Info),
        ActivationSource::Offline => ("离线激活", Variant::Secondary),
    };
    let can_deactivate = matches!(activation.source, ActivationSource::Online)
        && activation.license_key.is_some()
        && !app.is_busy();
    let disabled_reason = if !can_deactivate {
        if matches!(activation.source, ActivationSource::Offline) {
            Some("只有在线激活许可证可以取消激活")
        } else if app.is_busy() {
            Some("当前操作完成后才能取消激活")
        } else {
            Some("当前许可证没有可撤销的在线设备席位")
        }
    } else {
        None
    };
    let context = ui.ctx().clone();

    Card::new()
        .title("当前许可证")
        .description("已在本地验证并保存的许可证信息")
        .divider()
        .outline()
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(Badge::new(source.0).variant(source.1));
                if let Some(license) = &activation.license {
                    ui.add(
                        Badge::new(status_text(&license.effective_status))
                            .variant(status_variant(&license.effective_status)),
                    );
                } else {
                    ui.add(Badge::new("令牌已验证").variant(Variant::Success));
                }
            });

            ui.add_space(12.0);
            let mut details = DescriptionList::new().label_width(112.0);
            if let Some(license) = &activation.license {
                details = details
                    .item(
                        "产品",
                        format!("{} ({})", license.product.name, license.product.code),
                    )
                    .item("许可证状态", status_text(&license.effective_status))
                    .item("策略", license.policy.name.clone());
            } else {
                details = details
                    .item("产品 ID", wrap_identifier(&activation.claims.aud))
                    .item("策略 ID", wrap_identifier(&activation.claims.policy_id));
            }
            details = details
                .item("许可证 ID", wrap_identifier(&activation.claims.sub))
                .item("令牌有效期", format_time(activation.claims.exp));
            if let Some(expiry) = activation.claims.license_expires_at {
                details = details.item("许可证有效期", format_time(expiry));
            }
            details = details.item("激活时间", format_time(activation.activated_at));
            details.bordered().show(ui);

            ui.add_space(12.0);
            ui.add(Heading::new("授权功能").h4());
            if activation.claims.entitlements.is_empty() {
                ui.add(Label::new("未配置授权功能").muted());
            } else {
                ui.horizontal_wrapped(|ui| {
                    for entitlement in &activation.claims.entitlements {
                        Tag::new(entitlement).variant(Variant::Secondary).show(ui);
                    }
                });
            }

            ui.add_space(12.0);
            let response = ui.add(Button::danger("取消激活").disabled(!can_deactivate));
            if response.clicked() {
                app.deactivate(context.clone());
            } else if let Some(reason) = disabled_reason {
                response.on_disabled_hover_text(reason);
            }
        });
}

fn status_text(status: &str) -> String {
    match status.to_ascii_lowercase().as_str() {
        "active" => "有效".into(),
        "inactive" => "未启用".into(),
        "expired" => "已过期".into(),
        "suspended" => "已暂停".into(),
        _ => status.to_owned(),
    }
}

fn status_variant(status: &str) -> Variant {
    match status.to_ascii_lowercase().as_str() {
        "active" => Variant::Success,
        "inactive" | "expired" | "suspended" => Variant::Danger,
        _ => Variant::Warning,
    }
}

fn wrap_identifier(value: &str) -> String {
    let mut wrapped = String::with_capacity(value.len() + value.len() / 16);
    for (index, character) in value.chars().enumerate() {
        if index > 0 && (index % 16 == 0 || character == '-') {
            wrapped.push('\u{200b}');
        }
        wrapped.push(character);
    }
    wrapped
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
