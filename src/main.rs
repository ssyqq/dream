// #![windows_subsystem = "windows"]
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
            let mut fonts = FontDefinitions::default();
            
            // 添加字体
            #[cfg(target_os = "windows")]
            {   
                // fa solid
                fonts.font_data.insert(
                    "fa-solid".to_owned(),
                    egui::FontData::from_static(include_bytes!(
                        r"c:\USERS\AIMER\APPDATA\LOCAL\MICROSOFT\WINDOWS\FONTS\fa-solid-900.ttf"
                    )),
                );

                // harmonyos font 
                fonts.font_data.insert(
                    "harmonyos".to_owned(),
                    egui::FontData::from_static(include_bytes!(
                        r"c:\USERS\AIMER\APPDATA\LOCAL\MICROSOFT\WINDOWS\FONTS\HARMONYOS_SANS_SC_REGULAR.TTF"
                    )),
                );

                // JetBrains Mono Nerd Font
                fonts.font_data.insert(
                    "jetbrains".to_owned(),
                    egui::FontData::from_static(include_bytes!(
                        r"c:\USERS\AIMER\APPDATA\LOCAL\MICROSOFT\WINDOWS\FONTS\JETBRAINSMONONERDFONT-REGULAR.TTF"
                    )),
                );
            }
            // 完全覆盖默认字体族设置
            fonts.families.clear();  // 清除所有默认字体族
            
            // 设置新的字体族
            fonts.families.insert(
                FontFamily::Proportional,
                vec![
                    "harmonyos".to_owned(),
                    "fa-solid".to_owned(),
                ],
            );
            
            fonts.families.insert(
                FontFamily::Monospace,
                vec![
                    "jetbrains".to_owned(),
                    "harmonyos".to_owned(),
                ],
            );

            cc.egui_ctx.set_fonts(fonts);
            
            let app = ChatApp::new(runtime);
            Ok(Box::new(app) as Box<dyn eframe::App>)
        }),
    )
}
