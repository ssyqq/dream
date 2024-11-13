use eframe::egui::{self, RichText, ScrollArea, TextEdit, load::SizedTexture};
use crate::models::{ChatList, Message, Chat, ChatHistory};
use crate::config;
use crate::api;
use crate::utils;
use tokio::runtime::Runtime;
use reqwest::Client;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;
use eframe::egui::TextureHandle;
use log::{debug, error};
use uuid::Uuid;
use serde_json::json;
use rfd::FileDialog;
use image::GenericImageView;

pub struct ChatApp {
    pub input_text: String,
    pub chat_history: ChatHistory,
    pub api_key: String,
    pub runtime: Runtime,
    pub receiver: Option<mpsc::UnboundedReceiver<String>>,
    pub show_settings: bool,
    pub api_endpoint: String,
    pub model_name: String,
    pub system_prompt: String,
    pub temperature: f32,
    pub client: Client,
    pub chat_list: ChatList,
    pub previous_show_settings: bool,
    pub retry_enabled: bool,
    pub max_retries: i32,
    pub selected_image: Option<PathBuf>,
    pub texture_cache: HashMap<String, TextureHandle>,
}

impl Default for ChatApp {
    fn default() -> Self {
        // 修复 timeout 的类型问题
        let client = Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();

        // 读取配置文件
        let config = config::load_config();

        let mut app = Self {
            input_text: String::new(),
            chat_history: ChatHistory(Vec::new()),
            api_key: config.api_key,
            runtime: Runtime::new().unwrap(),
            receiver: None,
            show_settings: false,
            api_endpoint: config.api.endpoint,
            model_name: config.api.model,
            system_prompt: config.chat.system_prompt,
            temperature: config.chat.temperature as f32,
            client,
            chat_list: ChatList::default(),
            previous_show_settings: false,
            retry_enabled: config.chat.retry_enabled,
            max_retries: config.chat.max_retries as i32,
            selected_image: None,
            texture_cache: HashMap::new(),
        };
        
        // 如果没有任何对话，创建一个默认对话，但不选中它
        if app.chat_list.chats.is_empty() {
            let id = Uuid::new_v4().to_string();
            let new_chat = Chat {
                id: id.clone(),
                name: "新对话".to_string(),
                messages: Vec::new(),
                has_been_renamed: false,
            };
            app.chat_list.chats.insert(0, new_chat);
        }
        
        // 尝试加载聊天列表
        if let Err(e) = app.load_chat_list() {
            eprintln!("加载聊天列表失败: {}", e);
        }
        
        // 确保没有选中任何对话
        app.chat_list.current_chat_id = None;
        app.chat_history.0.clear();
        
        app
    }
}

impl ChatApp {
    fn save_config(&self, _frame: &mut eframe::Frame) -> Result<(), Box<dyn std::error::Error>> {
        debug!("正在保存配置...");
        let config = config::Config {
            api_key: self.api_key.clone(),
            api: config::ApiConfig {
                endpoint: self.api_endpoint.clone(),
                model: self.model_name.clone(),
            },
            chat: config::ChatConfig {
                system_prompt: self.system_prompt.clone(),
                temperature: self.temperature as f64,
                retry_enabled: self.retry_enabled,
                max_retries: self.max_retries as i64,
            },
        };
        
        config::save_config(&config).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    fn save_chat_list(&self) -> Result<(), Box<dyn std::error::Error>> {
        debug!("正在保存聊天列表...");
        let json = serde_json::to_string_pretty(&self.chat_list)?;
        std::fs::write("chat_list.json", json)?;
        debug!("聊天列表保存成功");
        Ok(())
    }

    fn load_chat_list(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(content) = std::fs::read_to_string("chat_list.json") {
            self.chat_list = serde_json::from_str(&content)?;
            // 加载后反转列表顺序
            self.chat_list.chats.reverse();
        }
        Ok(())
    }

    fn new_chat(&mut self) {
        debug!("创建新对话");
        let chat_count = self.chat_list.chats.len();
        let new_chat = Chat::new(format!("新对话 {}", chat_count + 1));
        let id = new_chat.id.clone();
        
        self.chat_list.chats.insert(0, new_chat);
        self.chat_list.current_chat_id = Some(id);
        self.chat_history.0.clear();
        
        if let Err(e) = self.save_chat_list() {
            error!("保存聊天列表失败: {}", e);
        }
    }

    async fn generate_title(&self, messages: &[Message]) -> Result<String, Box<dyn std::error::Error + Send>> {
        debug!("正在生成对话标题...");
        let content = messages.first()
            .map(|msg| msg.content.clone())
            .unwrap_or_default();

        let messages = vec![
            json!({
                "role": "system",
                "content": "请根据用户的输入生成一个简短的标题(不超过20个字),直接返回标题即可,不需要任何解释或额外的标点符号。"
            }),
            json!({
                "role": "user",
                "content": content
            }),
        ];

        let response = self.client
            .post(&self.api_endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&json!({
                "model": self.model_name,
                "messages": messages,
                "temperature": 0.7,
                "max_tokens": 60
            }))
            .send()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        let response_json = response.json::<serde_json::Value>().await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        let title = response_json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("新对话")
            .trim()
            .to_string();
            
        debug!("成功生成标题: {}", title);
        Ok(title)
    }

    fn send_message(&mut self) {
        let user_input = std::mem::take(&mut self.input_text);
        let image_path = self.selected_image.take();
        
        // 如果没有选中的聊天，创建一个新的
        if self.chat_list.current_chat_id.is_none() {
            self.new_chat();
        }

        debug!("准备发送消息");
        
        // 检查是否需要生成标题（在添加新消息之前）
        let should_generate_title = if let Some(current_id) = &self.chat_list.current_chat_id {
            let should = self.chat_list.chats
                .iter()
                .find(|c| &c.id == current_id)
                .map(|chat| !chat.has_been_renamed && chat.messages.is_empty())
                .unwrap_or(false);
            
            debug!("检查是否需要生成标题: {}", should);
            should
        } else {
            false
        };

        // 构建消息
        let mut messages = vec![
            json!({
                "role": "system",
                "content": self.system_prompt.clone()
            })
        ];

        // 添加历史消息
        for msg in &self.chat_history.0 {
            if let Ok(content) = msg.to_api_content() {
                messages.push(json!({
                    "role": msg.role,
                    "content": content
                }));
            } else {
                error!("处理历史消息失败");
            }
        }

        // 处理新消息
        let cached_image_path = if let Some(path) = image_path {
            match utils::copy_to_cache(&path) {
                Ok(cache_path) => Some(cache_path),
                Err(e) => {
                    error!("制图片到缓存失: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // 使用新的构造方法
        let new_message = Message::new_user(
            user_input,
            cached_image_path.map(|p| p.to_string_lossy().to_string()),
        );

        if let Ok(content) = new_message.to_api_content() {
            messages.push(json!({
                "role": "user",
                "content": content
            }));
        }

        self.chat_history.0.push(new_message);

        // 建立发送通道
        let (tx, rx) = mpsc::unbounded_channel();
        self.receiver = Some(rx);
        
        // 克隆需要的值
        let api_key = self.api_key.clone();
        let api_endpoint = self.api_endpoint.clone();
        let model_name = self.model_name.clone();
        let client = self.client.clone();
        let retry_enabled = self.retry_enabled;
        let max_retries = self.max_retries;

        // 在这里克隆两次 tx
        let msg_tx = tx.clone();
        let title_tx = tx;  // 原始的 tx 用于标题生成

        // 构建请求payload
        let payload = json!({
            "model": model_name.clone(),
            "messages": messages,
            "temperature": self.temperature,
            "stream": true
        });

        debug!("启动异步发送任务");
        // 在运行时中启动异步任务，使用 msg_tx
        self.runtime.spawn(async move {
            if let Err(e) = api::send_request(
                &client,
                &api_endpoint,
                &api_key,
                &payload,
                retry_enabled,
                max_retries,
                &msg_tx
            ).await {
                error!("发送请求失败: {:?}", e);
                let error_message = match e {
                    api::ApiError::TooManyRequests(_) => "请求频率限制，请稍后重试".to_string(),
                    api::ApiError::HttpError(res) => format!("API错误: {}", res.status()),
                    api::ApiError::Other(e) => format!("请求失败: {}", e),
                };
                let _ = msg_tx.send(error_message);
                let _ = msg_tx.send("__STREAM_DONE__".to_string());
            }
        });

        // 如果需要生成标题，使用 title_tx
        if should_generate_title {
            debug!("开始生成标题任务");
            let messages = self.chat_history.0.clone();
            let chat_id = self.chat_list.current_chat_id.clone().unwrap();
            let client = self.client.clone();
            let api_endpoint = self.api_endpoint.clone();
            let api_key = self.api_key.clone();
            let model_name = model_name.clone();
            let title_tx = title_tx.clone();
            
            self.runtime.spawn(async move {
                debug!("发送标题生成请求");
                let response = client
                    .post(&api_endpoint)
                    .header("Authorization", format!("Bearer {}", api_key))
                    .header("Content-Type", "application/json")
                    .json(&json!({
                        "model": model_name,
                        "messages": vec![
                            json!({
                                "role": "system",
                                "content": "请根据用户的输入生成一个简短的标题(不超过20个字),直接返回标题即可,不需要任何解释或额外的标点符号。"
                            }),
                            json!({
                                "role": "user",
                                "content": messages.first().map(|msg| msg.content.clone()).unwrap_or_default()
                            }),
                        ],
                        "temperature": 0.7,
                        "max_tokens": 60
                    }))
                    .send()
                    .await;

                let title = match response {
                    Ok(resp) => {
                        match resp.json::<serde_json::Value>().await {
                            Ok(json) => {
                                let title = json["choices"][0]["message"]["content"]
                                    .as_str()
                                    .map(|s| s.trim().to_string());
                                debug!("生成标题成功: {:?}", title);
                                title
                            }
                            Err(e) => {
                                error!("解析标题响应失败: {}", e);
                                None
                            }
                        }
                    }
                    Err(e) => {
                        error!("标题生成请求失败: {}", e);
                        None
                    }
                };

                if let Some(title) = title {
                    debug!("发送标题更新消息: {}", title);
                    let _ = title_tx.send(format!("__TITLE_UPDATE__{}:{}", chat_id, title));
                }
            });
        }
    }

    fn handle_message_selection(&mut self, messages: Vec<Message>) {
        self.chat_history.0 = messages;
    }

    fn handle_response(&mut self, response: String) {
        if self.chat_history.last_message_is_assistant() {
            self.chat_history.update_last_message(response);
        } else {
            self.chat_history.add_message(Message::new_assistant(response));
        }
    }

    fn ensure_image_loaded(&mut self, ui: &mut egui::Ui, path: &str) {
        if !self.texture_cache.contains_key(path) {
            if let Some(texture) = self.load_image(ui, path) {
                self.texture_cache.insert(path.to_string(), texture);
            }
        }
    }

    fn display_message(&mut self, ui: &mut egui::Ui, msg: &Message) {
        match msg.role.as_str() {
            "user" => {
                ui.label(RichText::new("You: ").strong());
                ui.label(&msg.content);
                
                if let Some(path) = &msg.image_path {
                    self.ensure_image_loaded(ui, path);
                    
                    if let Some(texture) = self.texture_cache.get(path) {
                        let max_display_size = 200.0;
                        let size = texture.size_vec2();
                        let scale = max_display_size / size.x.max(size.y);
                        let display_size = egui::vec2(size.x * scale, size.y * scale);
                        
                        let sized_texture = SizedTexture::new(texture.id(), display_size);
                        ui.add(egui::Image::new(sized_texture));
                    }
                }
            }
            "assistant" => {
                ui.label(RichText::new("AI: ").strong());
                ui.label(&msg.content);
            }
            _ => {}
        }
    }

    // 清理不再使用的纹理缓存
    fn clean_texture_cache(&mut self) {
        let mut used_paths = std::collections::HashSet::new();
        
        // 收集所有正在使用的图片路径
        for chat in &self.chat_list.chats {
            for msg in &chat.messages {
                if let Some(path) = &msg.image_path {
                    used_paths.insert(path.clone());
                }
            }
        }
        
        // 移除未使用的纹理
        self.texture_cache.retain(|path, _| used_paths.contains(path));
    }

    fn load_image(&mut self, ui: &mut egui::Ui, path: &str) -> Option<egui::TextureHandle> {
        if let Ok(image_bytes) = std::fs::read(path) {
            if let Ok(image) = image::load_from_memory(&image_bytes) {
                let dimensions = image.dimensions();
                let max_size = 800;
                let (width, height) = if dimensions.0 > max_size || dimensions.1 > max_size {
                    let scale = max_size as f32 / dimensions.0.max(dimensions.1) as f32;
                    ((dimensions.0 as f32 * scale) as u32, 
                     (dimensions.1 as f32 * scale) as u32)
                } else {
                    dimensions
                };
                
                let rgba_image = image.into_rgba8();
                let resized = image::imageops::resize(
                    &rgba_image,
                    width,
                    height,
                    image::imageops::FilterType::Triangle
                );
                
                let pixels = resized.as_raw();
                let texture = ui.ctx().load_texture(
                    format!("img_{}", path.replace("/", "_")),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [width as _, height as _],
                        pixels,
                    ),
                    egui::TextureOptions::default(),
                );
                
                return Some(texture);
            }
        }
        None
    }
}

impl Clone for ChatApp {
    fn clone(&self) -> Self {
        Self {
            input_text: self.input_text.clone(),
            chat_history: self.chat_history.clone(),
            api_key: self.api_key.clone(),
            runtime: Runtime::new().unwrap(), // 创建新的 Runtime
            receiver: None, // 不克隆 receiver
            show_settings: self.show_settings,
            api_endpoint: self.api_endpoint.clone(),
            model_name: self.model_name.clone(),
            system_prompt: self.system_prompt.clone(),
            temperature: self.temperature,
            client: self.client.clone(),
            chat_list: self.chat_list.clone(),
            previous_show_settings: self.previous_show_settings,
            retry_enabled: self.retry_enabled,
            max_retries: self.max_retries,
            selected_image: self.selected_image.clone(),
            texture_cache: self.texture_cache.clone(),
        }
    }
}

impl eframe::App for ChatApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::SidePanel::left("chat_list_panel")
            .default_width(200.0)
            .show(ctx, |ui| {
                let available_height = ui.available_height();
                
                egui::Frame::none()
                    .fill(ui.style().visuals.window_fill())
                    .show(ui, |ui| {
                        ui.set_min_height(available_height);
                        
                        ui.vertical(|ui| {
                            // 顶部区域
                            ui.horizontal(|ui| {
                                if ui.button("➕").clicked() {
                                    self.new_chat();
                                }
                            });
                            
                            ui.separator();
                            
                            // 聊天列表区域 - 设置为充满剩余空间
                            ScrollArea::vertical()
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    let mut selected_messages = None;
                                    let mut selected_id = None;
                                    
                                    // 创建一个反向迭代器来倒序显示聊天列表
                                    for chat in self.chat_list.chats.iter().rev() {
                                        let is_selected = self.chat_list.current_chat_id
                                            .as_ref()
                                            .map_or(false, |id| id == &chat.id);
                                        
                                        ui.horizontal(|ui| {
                                            ui.set_min_height(24.0);
                                            
                                            let response = ui.selectable_label(
                                                is_selected,
                                                RichText::new(&chat.name)
                                            );
                                            
                                            if response.clicked() {
                                                selected_id = Some(chat.id.clone());
                                                selected_messages = Some(chat.messages.clone());
                                            }
                                        });
                                    }
                                    
                                    if let Some(id) = selected_id {
                                        self.chat_list.current_chat_id = Some(id);
                                        if let Some(messages) = selected_messages {
                                            self.handle_message_selection(messages);
                                        }
                                    }
                                });
                            
                            // 底部齿轮按钮
                            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                                ui.add_space(4.0);
                                if ui.button("⚙").clicked() {
                                    self.show_settings = !self.show_settings;
                                }
                            });
                        });
                    });

                // 将删除快捷键检查移到这里，在 SidePanel 的上下文中
                if ui.ui_contains_pointer() && 
                   ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Backspace)) {
                    if let Some(current_id) = self.chat_list.current_chat_id.clone() {
                        // 如果删除的是当前选中的对话，清空聊天历史
                        self.chat_history.0.clear();
                        self.chat_list.current_chat_id = None;
                        
                        // 从列表中移除对话
                        self.chat_list.chats.retain(|chat| chat.id != current_id);
                        
                        // 如果删除后没有对话了，创建一个新的
                        if self.chat_list.chats.is_empty() {
                            self.new_chat();
                        } else {
                            // 如果当前没有选中的对话，选中第一个
                            if let Some(first_chat) = self.chat_list.chats.first() {
                                self.chat_list.current_chat_id = Some(first_chat.id.clone());
                                self.handle_message_selection(first_chat.messages.clone());
                            }
                        }
                        // 保存更改
                        let _ = self.save_chat_list();
                    }
                }
            });

        // 修改中央面板，移除顶部的连续聊天项
        egui::CentralPanel::default().show(ctx, |ui| {
            let total_height = ui.available_height();
            let input_height = 80.0;
            let history_height = total_height - input_height;
            
            ui.vertical(|ui| {
                // 设置面板现在显示在左侧面板上
                if self.show_settings {
                    // 只在设置首次打开时打印日志
                    if !self.previous_show_settings {
                        debug!("打开设置面板");
                    }
                    
                    egui::Window::new("设置")
                        .collapsible(false)
                        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                        .show(ctx, |ui| {
                            let mut config_changed = false;
                            
                            egui::Grid::new("settings_grid")
                                .num_columns(2)
                                .spacing([8.0, 4.0])
                                .show(ui, |ui| {
                                    // API Key 设置
                                    ui.label("API Key:");
                                    if ui.add(TextEdit::singleline(&mut self.api_key)
                                        .password(true)
                                        .desired_width(ui.available_width() - 60.0)).changed() {
                                        config_changed = true;
                                    }
                                    ui.end_row();

                                    // API 端点设置
                                    ui.label("API 端点:");
                                    if ui.add(TextEdit::singleline(&mut self.api_endpoint)
                                        .desired_width(ui.available_width() - 60.0)).changed() {
                                        config_changed = true;
                                    }
                                    ui.end_row();

                                    // 模型名称设置
                                    ui.label("模型名称:");
                                    if ui.add(TextEdit::singleline(&mut self.model_name)
                                        .desired_width(ui.available_width() - 60.0)).changed() {
                                        config_changed = true;
                                    }
                                    ui.end_row();

                                    // System Prompt 设置
                                    ui.label("系统提示:");
                                    if ui.add(TextEdit::multiline(&mut self.system_prompt)
                                        .desired_rows(2)
                                        .desired_width(ui.available_width() - 60.0)).changed() {
                                        config_changed = true;
                                    }
                                    ui.end_row();

                                    // Temperature 设置
                                    ui.label("Temperature:");
                                    if ui.add(egui::Slider::new(&mut self.temperature, 0.0..=2.0)
                                        .step_by(0.1)).changed() {
                                        config_changed = true;
                                    }
                                    ui.end_row();

                                    // 添加重试设置
                                    ui.label("启用重试:");
                                    if ui.checkbox(&mut self.retry_enabled, "").changed() {
                                        config_changed = true;
                                    }
                                    ui.end_row();

                                    // 最大重试次数设置
                                    ui.label("最大重试次数:");
                                    if ui.add(egui::Slider::new(&mut self.max_retries, 1..=20)).changed() {
                                        config_changed = true;
                                    }
                                    ui.end_row();
                                });
                            
                            if config_changed {
                                debug!("配置已更改，正在保存");
                                if let Err(e) = self.save_config(frame) {
                                    error!("保存配置失败: {}", e);
                                }
                            }
                        });
                } else if self.previous_show_settings {
                    // 当设置面板关闭时打印日志
                    debug!("关闭设置面板");
                }

                // 更新上一次的状态
                self.previous_show_settings = self.show_settings;

                // 聊天历史记录区域
                ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .stick_to_bottom(true)
                    .max_height(history_height)
                    .show(ui, |ui| {
                        let messages = self.chat_history.0.clone();
                        for (i, msg) in messages.iter().enumerate() {
                            if i > 0 {
                                ui.add_space(4.0);
                                ui.separator();
                                ui.add_space(4.0);
                            }
                            self.display_message(ui, msg);
                        }
                    });

                // 输入区
                ui.horizontal(|ui| {
                    let available_width = ui.available_width();
                    
                    // 修改入区域的布局
                    ui.vertical(|ui| {
                        // 图片上传按钮和文件名显示放在上方
                        ui.horizontal(|ui| {
                            if ui.button("📎").clicked() {
                                if let Some(path) = FileDialog::new()
                                    .add_filter("图片", &["png", "jpg", "jpeg"])
                                    .pick_file() 
                                {
                                    self.selected_image = Some(path);
                                }
                            }
                            
                            // 显示图片文件名
                            let mut should_clear_image = false;
                            if let Some(path) = &self.selected_image {
                                if let Some(file_name) = path.file_name() {
                                    if let Some(name) = file_name.to_str() {
                                        ui.label(name);
                                        if ui.button("❌").clicked() {
                                            should_clear_image = true;
                                        }
                                    }
                                }
                            }
                            if should_clear_image {
                                self.selected_image = None;
                            }
                        });

                        // 输入框和发送按钮在下方
                        ui.horizontal(|ui| {
                            let text_edit = TextEdit::multiline(&mut self.input_text)
                                .desired_rows(3)
                                .min_size(egui::vec2(available_width - 50.0, 60.0));
                            
                            let text_edit_response = ui.add(text_edit);
                            
                            if ui.add_sized(
                                [40.0, 60.0],
                                egui::Button::new("➤")
                            ).clicked() || (ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
                                && text_edit_response.has_focus())
                            {
                                if !self.input_text.is_empty() || self.selected_image.is_some() {
                                    self.send_message();
                                }
                            }
                        });
                    });
                });
            });

            // 处理主消息接收器
            let mut responses = Vec::new();
            if let Some(receiver) = &mut self.receiver {
                while let Ok(response) = receiver.try_recv() {
                    responses.push(response);
                }
            }

            for response in responses {
                match response.as_str() {
                    s if s.starts_with("__TITLE_UPDATE__") => {
                        debug!("收到标题更新消息: {}", s);
                        if let Some(remaining) = s.strip_prefix("__TITLE_UPDATE__") {
                            let parts: Vec<&str> = remaining.splitn(2, ':').collect();
                            if parts.len() == 2 {
                                let chat_id = parts[0];
                                let title = parts[1];
                                debug!("正在更新标题 - chat_id: {}, title: {}", chat_id, title);
                                if let Some(chat) = self.chat_list.chats
                                    .iter_mut()
                                    .find(|c| c.id == chat_id)
                                {
                                    debug!("找到对应的聊天，更新标题");
                                    chat.name = title.to_string();
                                    chat.has_been_renamed = true;
                                    if let Err(e) = self.save_chat_list() {
                                        error!("保存聊天列表失败: {}", e);
                                    } else {
                                        debug!("标题更新成功并保存");
                                    }
                                } else {
                                    debug!("未找到对应的聊天: {}", chat_id);
                                }
                            }
                        }
                    }
                    "__STREAM_DONE__" => {
                        debug!("流式响应完成");
                        if let Some(current_id) = &self.chat_list.current_chat_id {
                            if let Some(chat) = self.chat_list.chats
                                .iter_mut()
                                .find(|c| &c.id == current_id)
                            {
                                chat.messages = self.chat_history.0.clone();
                                let _ = self.save_chat_list();
                            }
                        }
                    }
                    _ => {
                        self.handle_response(response);
                    }
                }
            }
        });
    }
} 