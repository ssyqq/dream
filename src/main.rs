mod models;
mod api;
mod config;
mod utils;
mod ui;

use ui::ChatApp;
use eframe::egui::{self, FontDefinitions, FontFamily};
use tokio::runtime::Runtime;

fn main() -> Result<(), eframe::Error> {
    utils::setup_logger();
    
    // 创建一个多线程运行时
    let runtime = Runtime::new().unwrap();
    
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([600.0, 600.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "ChatGPT Client",
        options,
        Box::new(move |cc| {
            // 配置字体
            let mut fonts = FontDefinitions::default();
            
            // 根据操作系统添加不同的中文字体
            #[cfg(target_os = "macos")]
            fonts.font_data.insert(
                "chinese_font".to_owned(),
                egui::FontData::from_static(include_bytes!(
                    "/System/Library/Fonts/STHeiti Light.ttc"
                )),
            );

            #[cfg(target_os = "windows")]
            fonts.font_data.insert(
                "chinese_font".to_owned(),
                egui::FontData::from_static(include_bytes!(
                    "C:\\Windows\\Fonts\\msyh.ttc"
                )),
            );

            // 将中文字体设置为优先字体
            fonts.families
                .get_mut(&FontFamily::Proportional)
                .unwrap()
                .insert(0, "chinese_font".to_owned());

            // 设置字体
            cc.egui_ctx.set_fonts(fonts);
            
            // 创建应用实例
            let app = ChatApp::new(runtime);
            Ok(Box::new(app) as Box<dyn eframe::App>)
        }),
    )
}
