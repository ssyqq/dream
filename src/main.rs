use eframe::egui;
use eframe::egui::load::SizedTexture;
use egui::{RichText, ScrollArea, TextEdit, ViewportBuilder};
use eframe::egui::{FontDefinitions, FontFamily};
use reqwest::Client;
use serde_json::{json, Value as JsonValue};
use tokio::runtime::Runtime;
use std::fs;
use toml::Value as TomlValue;
use futures_util::StreamExt;
use tokio::sync::mpsc;
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use log::{debug, error, info};
use chrono::Local;
use env_logger::Builder;
use std::io::Write;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rfd::FileDialog;
use std::path::PathBuf;
use std::collections::HashMap;
use std::path::Path;
use image::GenericImageView;

#[derive(Serialize, Deserialize, Clone)]
struct Message {
    role: String,  // "user" 或 "assistant"
    content: String,
    image_path: Option<String>,  // 可选的图片路径
}

#[derive(Serialize, Deserialize, Clone)]
struct ChatHistory(Vec<Message>);

#[derive(Serialize, Deserialize, Clone)]
struct Chat {
    id: String,
    name: String,
    messages: Vec<Message>,
    has_been_renamed: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct ChatList {
    chats: Vec<Chat>,
    current_chat_id: Option<String>,
}

impl Default for ChatList {
    fn default() -> Self {
        Self {
            chats: Vec::new(),
            current_chat_id: None,
        }
    }
}

struct ChatApp {
    input_text: String,
    chat_history: ChatHistory,
    api_key: String,
    is_sending: bool,  // 新增：用于显示发送状态
    runtime: Runtime,
    receiver: Option<mpsc::UnboundedReceiver<String>>,  // 改为 tokio 的 mpsc
    show_settings: bool,
    api_endpoint: String,
    model_name: String,
    system_prompt: String,
    temperature: f32,
    client: Client,  // 添加这个字段
    chat_list: ChatList,  // 新增字段
    previous_show_settings: bool,  // 新增字段
    retry_enabled: bool,     // 是否启用重试
    max_retries: i32,       // 最大重试次数
    selected_image: Option<PathBuf>,  // 新增：当前选择的图片路径
    texture_cache: HashMap<String, egui::TextureHandle>,
    current_messages: Vec<(String, String)>, // 用于显示的消息缓存
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
        let config = fs::read_to_string("dream.toml").unwrap_or_else(|_| String::new());
        let config: TomlValue = toml::from_str(&config).unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new()));

        let mut app = Self {
            input_text: String::new(),
            chat_history: ChatHistory(Vec::new()),
            api_key: config.get("api_key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            is_sending: false,
            runtime: Runtime::new().unwrap(),
            receiver: None,
            show_settings: false,
            api_endpoint: config.get("api")
                .and_then(|v| v.get("endpoint"))
                .and_then(|v| v.as_str())
                .unwrap_or("https://api.openai.com/v1/chat/completions")
                .to_string(),
            model_name: config.get("api")
                .and_then(|v| v.get("model"))
                .and_then(|v| v.as_str())
                .unwrap_or("gpt-3.5-turbo")
                .to_string(),
            system_prompt: config.get("chat")
                .and_then(|v| v.get("system_prompt"))
                .and_then(|v| v.as_str())
                .unwrap_or("你是一个有帮助的助手。")
                .to_string(),
            temperature: config.get("chat")
                .and_then(|v| v.get("temperature"))
                .and_then(|v| v.as_float())
                .map(|f| f as f32)
                .unwrap_or(0.7),
            client,
            chat_list: ChatList::default(),
            previous_show_settings: false,  // 初始化新字段
            retry_enabled: config.get("chat")
                .and_then(|v| v.get("retry_enabled"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            max_retries: config.get("chat")
                .and_then(|v| v.get("max_retries"))
                .and_then(|v| v.as_integer())
                .unwrap_or(10) as i32,
            selected_image: None,  // 初始化新字段
            texture_cache: HashMap::new(),
            current_messages: Vec::new(),
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

// 在 ChatApp 实现块之前添加这些辅助函数
fn ensure_cache_dir() -> std::io::Result<PathBuf> {
    let cache_dir = PathBuf::from(".cache/images");
    std::fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir)
}

fn copy_to_cache(source_path: &Path) -> std::io::Result<PathBuf> {
    let cache_dir = ensure_cache_dir()?;
    let file_name = format!("{}.jpg", Uuid::new_v4());
    let cache_path = cache_dir.join(&file_name);
    
    // 读取源图片
    let img = image::open(source_path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    
    // 转换为 JPEG 并保存到缓存目录
    img.save(&cache_path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    
    Ok(cache_path)
}

fn get_image_base64(path: &Path) -> std::io::Result<String> {
    let image_data = std::fs::read(path)?;
    Ok(BASE64.encode(&image_data))
}

// 在 Message 结构体中添加一个方法
impl Message {
    fn to_api_content(&self) -> std::io::Result<JsonValue> {
        match &self.image_path {
            Some(path) => {
                let base64_image = get_image_base64(Path::new(path))?;
                Ok(json!([
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:image/jpeg;base64,{}", base64_image)
                        }
                    },
                    {
                        "type": "text",
                        "text": self.content
                    }
                ]))
            }
            None => Ok(json!(self.content))
        }
    }
}

impl ChatApp {
    fn save_config(&self, _frame: &mut eframe::Frame) -> Result<(), Box<dyn std::error::Error>> {
        debug!("正在保存配置...");
        let mut config = toml::map::Map::new();
        
        // API 相关配置
        let mut api = toml::map::Map::new();
        api.insert("endpoint".to_string(), toml::Value::String(self.api_endpoint.clone()));
        api.insert("model".to_string(), toml::Value::String(self.model_name.clone()));
        config.insert("api".to_string(), toml::Value::Table(api));
        
        // Chat 关配置
        let mut chat = toml::map::Map::new();
        chat.insert("system_prompt".to_string(), toml::Value::String(self.system_prompt.clone()));
        chat.insert("temperature".to_string(), toml::Value::Float(self.temperature as f64));
        chat.insert("retry_enabled".to_string(), toml::Value::Boolean(self.retry_enabled));
        chat.insert("max_retries".to_string(), toml::Value::Integer(self.max_retries as i64));
        config.insert("chat".to_string(), toml::Value::Table(chat));
        
        // API Key
        config.insert("api_key".to_string(), toml::Value::String(self.api_key.clone()));
        
        // 配置转换为 TOML 字符串
        let toml_string = toml::to_string_pretty(&toml::Value::Table(config))?;
        
        // 写入文件
        match fs::write("dream.toml", toml_string) {
            Ok(_) => {
                debug!("配置保存成功");
                Ok(())
            }
            Err(e) => {
                error!("保存配置失败: {}", e);
                Err(Box::new(e))
            }
        }
    }

    fn save_chat_list(&self) -> Result<(), Box<dyn std::error::Error>> {
        debug!("正在保存聊天列表...");
        let json = serde_json::to_string_pretty(&self.chat_list)?;
        match fs::write("chat_list.json", json) {
            Ok(_) => {
                debug!("聊天列表保存成功");
                Ok(())
            }
            Err(e) => {
                error!("保存聊天列表失败: {}", e);
                Err(Box::new(e))
            }
        }
    }

    fn load_chat_list(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(content) = fs::read_to_string("chat_list.json") {
            self.chat_list = serde_json::from_str(&content)?;
            // 加载反转列表顺序
            self.chat_list.chats.reverse();
        }
        Ok(())
    }

    fn new_chat(&mut self) {
        debug!("创建新对话");
        let id = Uuid::new_v4().to_string();
        let chat_count = self.chat_list.chats.len();
            
        let new_chat = Chat {
            id: id.clone(),
            name: format!("新对话 {}", chat_count + 1),
            messages: Vec::new(),
            has_been_renamed: false,
        };
        // 将新对话插入到列表开头而不是末尾
        self.chat_list.chats.insert(0, new_chat);
        self.chat_list.current_chat_id = Some(id);
        self.chat_history.0.clear();
        if let Err(e) = self.save_chat_list() {
            error!("保存聊天列表失败: {}", e);
        }
    }

    async fn generate_title(&self, messages: &[Message]) -> Result<String, Box<dyn std::error::Error + Send>> {
        debug!("正在生成对话标题...");
        // 构建用于生成标题的提示
        let content = messages.first()
            .map(|msg| msg.content.clone())
            .unwrap_or_default();

        let messages = vec![
            json!({
                "role": "system",
                "content": "请根据户的输入生成一个简短的标题(不超过20个字),直接返回标题即可,不需要任何解释或额外的标点符号。"
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

        match response.json::<JsonValue>().await {
            Ok(response_json) => {
                let title = response_json["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("新对话")
                    .trim()
                    .to_string();
                debug!("成功生成标题: {}", title);
                Ok(title)
            }
            Err(e) => {
                error!("生成标题失败: {}", e);
                Err(Box::new(e))
            }
        }
    }

    fn send_message(&mut self) {
        let user_input = std::mem::take(&mut self.input_text);
        let image_path = self.selected_image.take();
        
        // 如果没有选中的聊天，创建一个新的
        if self.chat_list.current_chat_id.is_none() {
            self.new_chat();
        }

        debug!("准备发送消息");
        self.is_sending = true;
        
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
            match copy_to_cache(&path) {
                Ok(cache_path) => Some(cache_path),
                Err(e) => {
                    error!("复制图片到缓存失败: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // 添加用户新消息
        let new_message = Message {
            role: "user".to_string(),
            content: user_input,
            image_path: cached_image_path.map(|p| p.to_string_lossy().to_string()),
        };

        if let Ok(content) = new_message.to_api_content() {
            messages.push(json!({
                "role": "user",
                "content": content
            }));
        }

        self.chat_history.0.push(new_message);

        // 建发送通道
        let (tx, rx) = mpsc::unbounded_channel();
        self.receiver = Some(rx);
        
        // 克隆需要的值
        let api_key = self.api_key.clone();
        let api_endpoint = self.api_endpoint.clone();
        let model_name = self.model_name.clone();
        let client = self.client.clone();
        let retry_enabled = self.retry_enabled;
        let max_retries = self.max_retries;

        // 构建请payload
        let payload = json!({
            "model": model_name,
            "messages": messages,
            "temperature": self.temperature,
            "stream": true
        });

        debug!("启动异步发送任");
        // 在运行时中启动异任务
        self.runtime.spawn(async move {
            if let Err(e) = send_request(
                &client,
                &api_endpoint,
                &api_key,
                &payload,
                retry_enabled,
                max_retries,
                &tx
            ).await {
                error!("发送请求失败: {:?}", e);
                let error_message = match e {
                    ApiError::TooManyRequests(_) => "请求频率限制，请后重试".to_string(),
                    ApiError::HttpError(res) => format!("API错误: {}", res.status()),
                    ApiError::Other(e) => format!("请求失败: {}", e),
                };
                let _ = tx.send(error_message);
                let _ = tx.send("__STREAM_DONE__".to_string());
            }
        });
    }

    fn handle_message_selection(&mut self, messages: Vec<Message>) {
        self.chat_history.0 = messages;
    }

    fn display_message(&mut self, ui: &mut egui::Ui, msg: &Message) {
        match msg.role.as_str() {
            "user" => {
                ui.label(egui::RichText::new("You: ").strong());
                ui.label(&msg.content);
                
                // 如果有图片，显示图片
                if let Some(path) = &msg.image_path {
                    let texture = self.texture_cache.entry(path.clone())
                        .or_insert_with(|| {
                            if let Ok(image_bytes) = std::fs::read(path) {
                                if let Ok(image) = image::load_from_memory(&image_bytes) {
                                    use image::GenericImageView;
                                    
                                    let dimensions = image.dimensions();
                                    let max_size = 800;
                                    let (width, height) = if dimensions.0 > max_size || dimensions.1 > max_size {
                                        let scale = max_size as f32 / dimensions.0.max(dimensions.1) as f32;
                                        ((dimensions.0 as f32 * scale) as u32, 
                                         (dimensions.1 as f32 * scale) as u32)
                                    } else {
                                        dimensions
                                    };
                                    
                                    // 先转换为 RGBA8
                                    let rgba_image = image.into_rgba8();
                                    // 然后调整大小
                                    let resized = image::imageops::resize(
                                        &rgba_image,
                                        width,
                                        height,
                                        image::imageops::FilterType::Triangle
                                    );
                                    
                                    // 确保图片数据大小正确
                                    let pixels = resized.as_raw();
                                    let expected_size = (width * height * 4) as usize;
                                    if pixels.len() != expected_size {
                                        // 如果大小不匹配，返回错误纹理
                                        return ui.ctx().load_texture(
                                            "error_texture",
                                            egui::ColorImage::new([16, 16], egui::Color32::RED),
                                            egui::TextureOptions::default(),
                                        );
                                    }
                                    
                                    ui.ctx().load_texture(
                                        format!("img_{}", path.replace("/", "_")),
                                        egui::ColorImage::from_rgba_unmultiplied(
                                            [width as _, height as _],
                                            pixels,
                                        ),
                                        egui::TextureOptions::default(),
                                    )
                                } else {
                                    ui.ctx().load_texture(
                                        "error_texture",
                                        egui::ColorImage::new([16, 16], egui::Color32::RED),
                                        egui::TextureOptions::default(),
                                    )
                                }
                            } else {
                                ui.ctx().load_texture(
                                    "error_texture",
                                    egui::ColorImage::new([16, 16], egui::Color32::RED),
                                    egui::TextureOptions::default(),
                                )
                            }
                        });

                    let max_display_size = 200.0;
                    let size = texture.size_vec2();
                    let scale = max_display_size / size.x.max(size.y);
                    let display_size = egui::vec2(size.x * scale, size.y * scale);
                    
                    let sized_texture = SizedTexture::new(texture.id(), display_size);
                    ui.add(egui::Image::new(sized_texture));
                }
            }
            "assistant" => {
                ui.label(egui::RichText::new("AI: ").strong());
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

    fn handle_response(&mut self, response: String) {
        if let Some(last_msg) = self.chat_history.0.last_mut() {
            if last_msg.role == "assistant" {
                last_msg.content = response;
            } else {
                self.chat_history.0.push(Message {
                    role: "assistant".to_string(),
                    content: response,
                    image_path: None,
                });
            }
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
                            
                            // 聊天列表区域 - 设置为充剩余空间
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

                // 只有当鼠标在聊天列表面板上时，才检查删除快捷键
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
                            // 如果当前没有选中的对话，中第一个
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

        // 修改中央面，移除顶部的连续聊天选项
        egui::CentralPanel::default().show(ctx, |ui| {
            let total_height = ui.available_height();
            let input_height = 80.0;
            let history_height = total_height - input_height;
            
            ui.vertical(|ui| {
                // 设置面板现在显示在左侧面板上
                if self.show_settings {
                    // 只在设置首次打开打印日志
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

                                    // 名称设置
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
                                    ui.label("启重试:");
                                    if ui.checkbox(&mut self.retry_enabled, "").changed() {
                                        config_changed = true;
                                    }
                                    ui.end_row();

                                    // 最大重试次数置
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

                // 聊天历史记录区
                ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .stick_to_bottom(true)
                    .max_height(history_height)
                    .show(ui, |ui| {
                        let messages = self.chat_history.0.clone();
                        let texture_cache = &mut self.texture_cache;
                        
                        for (i, msg) in messages.iter().enumerate() {
                            if i > 0 {
                                ui.add_space(4.0);
                                ui.separator();
                                ui.add_space(4.0);
                            }
                            
                            match msg.role.as_str() {
                                "user" => {
                                    ui.label(egui::RichText::new("You: ").strong());
                                    ui.label(&msg.content);
                                    
                                    if let Some(path) = &msg.image_path {
                                        let texture = texture_cache.entry(path.clone())
                                            .or_insert_with(|| {
                                                if let Ok(image_bytes) = std::fs::read(path) {
                                                    if let Ok(image) = image::load_from_memory(&image_bytes) {
                                                        use image::GenericImageView;
                                                        
                                                        let dimensions = image.dimensions();
                                                        let max_size = 800;
                                                        let (width, height) = if dimensions.0 > max_size || dimensions.1 > max_size {
                                                            let scale = max_size as f32 / dimensions.0.max(dimensions.1) as f32;
                                                            ((dimensions.0 as f32 * scale) as u32, 
                                                             (dimensions.1 as f32 * scale) as u32)
                                                        } else {
                                                            dimensions
                                                        };
                                                        
                                                        // 先转换为 RGBA8
                                                        let rgba_image = image.into_rgba8();
                                                        // 然后调整大小
                                                        let resized = image::imageops::resize(
                                                            &rgba_image,
                                                            width,
                                                            height,
                                                            image::imageops::FilterType::Triangle
                                                        );
                                                        
                                                        // 确保图片数据大小正确
                                                        let pixels = resized.as_raw();
                                                        let expected_size = (width * height * 4) as usize;
                                                        if pixels.len() != expected_size {
                                                            // 如果大小不匹配，返回错误纹理
                                                            return ui.ctx().load_texture(
                                                                "error_texture",
                                                                egui::ColorImage::new([16, 16], egui::Color32::RED),
                                                                egui::TextureOptions::default(),
                                                            );
                                                        }
                                                        
                                                        ui.ctx().load_texture(
                                                            format!("img_{}", path.replace("/", "_")),
                                                            egui::ColorImage::from_rgba_unmultiplied(
                                                                [width as _, height as _],
                                                                pixels,
                                                            ),
                                                            egui::TextureOptions::default(),
                                                        )
                                                    } else {
                                                        ui.ctx().load_texture(
                                                            "error_texture",
                                                            egui::ColorImage::new([16, 16], egui::Color32::RED),
                                                            egui::TextureOptions::default(),
                                                        )
                                                    }
                                                } else {
                                                    ui.ctx().load_texture(
                                                        "error_texture",
                                                        egui::ColorImage::new([16, 16], egui::Color32::RED),
                                                        egui::TextureOptions::default(),
                                                    )
                                                }
                                            });

                                        let max_display_size = 200.0;
                                        let size = texture.size_vec2();
                                        let scale = max_display_size / size.x.max(size.y);
                                        let display_size = egui::vec2(size.x * scale, size.y * scale);
                                        
                                        let sized_texture = SizedTexture::new(texture.id(), display_size);
                                        ui.add(egui::Image::new(sized_texture));
                                    }
                                }
                                "assistant" => {
                                    ui.label(egui::RichText::new("AI: ").strong());
                                    ui.label(&msg.content);
                                }
                                _ => {}
                            }
                        }
                    });

                // 输入区域
                ui.horizontal(|ui| {
                    let available_width = ui.available_width();
                    
                    // 修改输入区域的布局
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
                                egui::Button::new(if self.is_sending { "⏳" } else { "➤" })
                            ).clicked() || (ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
                                && text_edit_response.has_focus())
                            {
                                if (!self.input_text.is_empty() || self.selected_image.is_some()) && !self.is_sending {
                                    self.send_message();
                                }
                            }
                        });
                    });
                });
            });

            // 处理主消息接收器
            let mut should_save = false;
            if let Some(receiver) = &mut self.receiver {
                while let Ok(response) = receiver.try_recv() {
                    match response.as_str() {
                        "__STREAM_DONE__" => {
                            debug!("流式响应完成");
                            self.is_sending = false;
                            // 保存当前对话的消息
                            if let Some(current_id) = &self.chat_list.current_chat_id {
                                if let Some(chat) = self.chat_list.chats
                                    .iter_mut()
                                    .find(|c| &c.id == current_id)
                                {
                                    chat.messages = self.chat_history.0.clone();
                                    should_save = true;
                                }
                            }
                        }
                        _ => {
                            // 在这里处理消息，避免借用冲突
                            if let Some(last_msg) = self.chat_history.0.last_mut() {
                                if last_msg.role == "assistant" {
                                    last_msg.content = response;
                                } else {
                                    self.chat_history.0.push(Message {
                                        role: "assistant".to_string(),
                                        content: response,
                                        image_path: None,
                                    });
                                }
                            }
                        }
                    }
                }
            }

            // 只在聊天列表更新时保存
            if should_save {
                let _ = self.save_chat_list();
            }
        });
    }
}

#[derive(Debug)]
enum ApiError {
    TooManyRequests(reqwest::Response),
    Other(reqwest::Error),
    HttpError(reqwest::Response),
}

async fn send_request(
    client: &Client,
    api_endpoint: &str,
    api_key: &str,
    payload: &serde_json::Value,
    retry_enabled: bool,
    max_retries: i32,
    tx: &mpsc::UnboundedSender<String>,
) -> Result<(), ApiError> {
    let mut retry_count = 0;
    loop {
        debug!("发送API请求 (重试次数: {})", retry_count);
        let response = client
            .post(api_endpoint)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(payload)
            .send()
            .await
            .map_err(ApiError::Other)?;

        if !response.status().is_success() {
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                if retry_enabled && retry_count < max_retries {
                    retry_count += 1;
                    debug!("遇到 429 错误，即进行第 {} 次重试", retry_count);
                    let _ = tx.send(format!("遇到频率限制，正在进行第 {} 次重试...", retry_count));
                    continue;  // 直接重试，不等待
                }
                return Err(ApiError::TooManyRequests(response));
            }
            return Err(ApiError::HttpError(response));
        }

        let mut stream = response.bytes_stream();
        let mut current_message = String::new();
        
        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    if let Ok(text) = String::from_utf8(chunk.to_vec()) {
                        debug!("收到原始数据: {}", text);
                        for line in text.lines() {
                            debug!("处理数行: {}", line);
                            if line.starts_with("data: ") {
                                let data = &line[6..];
                                if data == "[DONE]" {
                                    debug!("收到结束标记: [DONE]");
                                    let _ = tx.send("__STREAM_DONE__".to_string());
                                    return Ok(());
                                }
                                match serde_json::from_str::<JsonValue>(data) {
                                    Ok(json) => {
                                        if let Some(error) = json.get("error") {
                                            if retry_enabled && retry_count < max_retries {
                                                retry_count += 1;
                                                debug!("遇到API错误，立即进行第 {} 次重试", retry_count);
                                                let _ = tx.send(format!("遇到API错误，正在进行第 {} 次重试...", retry_count));
                                                continue;  // 直接重试，不等待
                                            } else {
                                                // 构建更详细的错误信息
                                                let error_msg = if let Some(metadata) = error.get("metadata") {
                                                    if let Some(raw) = metadata.get("raw") {
                                                        format!("API错误 (重试{}次后): {} - 详细信息: {}", 
                                                            retry_count,
                                                            error["message"].as_str().unwrap_or("未知错误"),
                                                            raw.as_str().unwrap_or(""))
                                                    } else {
                                                        format!("API错误 (重试{}次后): {}", 
                                                            retry_count,
                                                            error["message"].as_str().unwrap_or("未知错误"))
                                                    }
                                                } else {
                                                    format!("API错误 (重试{}后): {}", 
                                                        retry_count,
                                                        error["message"].as_str().unwrap_or("未知错误"))
                                                };
                                                
                                                error!("{}", error_msg);
                                                let _ = tx.send(error_msg);
                                                let _ = tx.send("__STREAM_DONE__".to_string());
                                                return Ok(());
                                            }
                                        }

                                        if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                                            current_message.push_str(content);
                                            let _ = tx.send(current_message.clone());
                                        }
                                    }
                                    Err(e) => {
                                        debug!("JSON解析失败: {} - 原始数据: {}", e, data);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("流式数据接收错误: {}", e);
                    if retry_enabled && retry_count < max_retries {
                        retry_count += 1;
                        debug!("遇到网络错误，立即进行第 {} 次重试", retry_count);
                        let _ = tx.send(format!("遇到网络错误，正在进行第 {} 次重试...", retry_count));
                        continue;  // 直接重试，不等待
                    }
                    return Err(ApiError::Other(e.into()));
                }
            }
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    // 初始化日志系统
    Builder::from_default_env()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .filter_level(log::LevelFilter::Debug)
        .init();

    info!("应用程启动");
    
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([600.0, 600.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "ChatGPT Client",
        options,
        Box::new(|cc| {
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

            // 将中文字体设置为优先体
            fonts.families
                .get_mut(&FontFamily::Proportional)
                .unwrap()
                .insert(0, "chinese_font".to_owned());

            // 设置字体
            cc.egui_ctx.set_fonts(fonts);
            
            Ok(Box::new(ChatApp::default()))
        }),
    )
}
