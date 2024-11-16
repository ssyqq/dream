// #![windows_subsystem = "windows"]
mod api;
mod config;
mod models;
mod ui;
mod utils;

use eframe::egui::{self, FontDefinitions, FontFamily};
use tokio::runtime::Runtime;
use ui::ChatApp;

fn main() -> Result<(), eframe::Error> {
    utils::setup_logger();

    let runtime = Runtime::new().unwrap();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([600.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "ChatGPT Client",
        options,
        Box::new(move |cc| {
            let mut fonts = FontDefinitions::default();

            fonts.families.clear(); // 清除所有默认字体族

            // 添加 fa solid 字体
            fonts.font_data.insert(
                "fa-solid".to_owned(),
                egui::FontData::from_static(include_bytes!(".././assets/fonts/fa6-900.otf")),
            );

            // 添加 PingFang SC 字体（macOS 系统字体）
            fonts.font_data.insert(
                "PingFang-SC".to_owned(),
                egui::FontData::from_static(include_bytes!("../assets/fonts/pfsc.otf")),
            );

            // 添加jetbrains mono字体
            fonts.font_data.insert(
                "jetbrains".to_owned(),
                egui::FontData::from_static(include_bytes!(
                    "../assets/fonts/jetbrains-regular.ttf"
                )),
            );

            fonts.families.insert(
                FontFamily::Proportional,
                vec![
                    "jetbrains".to_owned(),
                    "fa-solid".to_owned(),
                    "PingFang-SC".to_owned(),
                ],
            );

            fonts.families.insert(
                FontFamily::Monospace,
                vec![
                    "jetbrains".to_owned(),
                    "fa-solid".to_owned(),
                    "PingFang-SC".to_owned(),
                ],
            );

            cc.egui_ctx.set_fonts(fonts);

            let app = ChatApp::new(runtime);
            Ok(Box::new(app) as Box<dyn eframe::App>)
        }),
    )
}
