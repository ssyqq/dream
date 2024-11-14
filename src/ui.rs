use eframe::egui::{self, RichText, ScrollArea, TextEdit, load::SizedTexture};
use crate::models::{ChatList, Message, Chat, ChatHistory};
use crate::config;
use crate::api;
use crate::utils::{self, ImageError};
use tokio::runtime::Runtime;
use reqwest::Client;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;
use eframe::egui::TextureHandle;
use log::{debug, error};
use uuid::Uuid;
use serde_json::{json, Value as JsonValue};
use rfd::FileDialog;
use image::GenericImageView;

pub struct ChatApp {
    pub input_text: String,
    pub chat_history: ChatHistory,
    pub api_key: String,
    pub runtime: Runtime,
    pub runtime_handle: tokio::runtime::Handle,
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
    pub processing_image: Option<tokio::task::JoinHandle<Result<PathBuf, ImageError>>>,
    pub dark_mode: bool,
    pub available_models: Vec<String>,
    pub input_focus: bool,
}

impl Default for ChatApp {
    fn default() -> Self {
        // 创建运行时
        let runtime = Runtime::new().unwrap();
        let runtime_handle = runtime.handle().clone();
        
        // 修复 timeout 的类型问题
        let client = Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();

        // 读取配置文件并等待结果
        let config = runtime_handle.block_on(async {
            config::load_config().await
        });

        let mut app = Self {
            input_text: String::new(),
            chat_history: ChatHistory(Vec::new()),
            api_key: config.api_key,
            runtime,
            runtime_handle,
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
            processing_image: None,
            dark_mode: config.chat.dark_mode,
            available_models: config.api.available_models,
            input_focus: true,
        };
        
        // 先尝试加载聊天列表
        if let Err(e) = app.load_chat_list() {
            eprintln!("加载聊天列表失败: {}", e);
        }
        
        // 只有在加载后聊天列表仍为空时，才创建默认对话
        if app.chat_list.chats.is_empty() {
            let id = Uuid::new_v4().to_string();
            let new_chat = Chat {
                id: id.clone(),
                name: "新对话".to_string(),
                messages: Vec::new(),
                has_been_renamed: false,
            };
            app.chat_list.chats.insert(0, new_chat);
            app.chat_list.current_chat_id = Some(id);
        }
        
        // 确保没有选中任何对话
        app.chat_list.current_chat_id = None;
        app.chat_history.0.clear();
        
        app
    }
}

impl ChatApp {
    pub fn new(runtime: Runtime) -> Self {
        debug!("创建新的 ChatApp 实例");
        let handle = runtime.handle().clone();
        
        // 修复 timeout 的类型问题
        debug!("初始化 HTTP 客户端");
        let client = Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();

        // 读取配置文件并等待结果
        debug!("加载配置文件");
        let config = handle.block_on(async {
            config::load_config().await
        });
        debug!("配置加载完成");

        let mut app = Self {
            input_text: String::new(),
            chat_history: ChatHistory(Vec::new()),
            api_key: config.api_key,
            runtime,
            runtime_handle: handle,
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
            processing_image: None,
            dark_mode: config.chat.dark_mode,
            available_models: config.api.available_models,
            input_focus: true,
        };
        
        // 先尝试加载聊天列表
        if let Err(e) = app.load_chat_list() {
            eprintln!("加载聊天列表失败: {}", e);
        }
        
        // 只有在加载后聊天列表仍为空时，才创建默认对话
        if app.chat_list.chats.is_empty() {
            let id = Uuid::new_v4().to_string();
            let new_chat = Chat {
                id: id.clone(),
                name: "新对话".to_string(),
                messages: Vec::new(),
                has_been_renamed: false,
            };
            app.chat_list.chats.insert(0, new_chat);
            app.chat_list.current_chat_id = Some(id);
        }
        
        // 确保没有选中任何对话
        app.chat_list.current_chat_id = None;
        app.chat_history.0.clear();
        
        debug!("ChatApp 实例创建完成");
        app
    }

    fn save_config(&self, _frame: &mut eframe::Frame) -> Result<(), Box<dyn std::error::Error>> {
        debug!("正在保存配置...");
        let config = config::Config {
            api_key: self.api_key.clone(),
            api: config::ApiConfig {
                endpoint: self.api_endpoint.clone(),
                model: self.model_name.clone(),
                available_models: self.available_models.clone(),
            },
            chat: config::ChatConfig {
                system_prompt: self.system_prompt.clone(),
                temperature: self.temperature as f64,
                retry_enabled: self.retry_enabled,
                max_retries: self.max_retries as i64,
                dark_mode: self.dark_mode,
            },
        };
        
        // 使用 block_on 等待异步保存完成
        self.runtime_handle.block_on(async {
            config::save_config(&config).await
        }).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn save_chat_list_async(&self) -> Result<(), Box<dyn std::error::Error>> {
        debug!("正在保存天列表...");
        let json = serde_json::to_string_pretty(&self.chat_list)?;
        tokio::fs::write("chat_list.json", json).await?;
        debug!("聊天列表保存成功");
        Ok(())
    }

    fn save_chat_list(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.runtime_handle.block_on(async {
            self.save_chat_list_async().await
        })
    }

    async fn load_chat_list_async(chat_list: &mut ChatList) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(content) = tokio::fs::read_to_string("chat_list.json").await {
            *chat_list = serde_json::from_str(&content)?;
            // 加载后反转列表顺序
            chat_list.chats.reverse();
        }
        Ok(())
    }

    fn load_chat_list(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut chat_list = self.chat_list.clone();
        self.runtime_handle.block_on(async {
            Self::load_chat_list_async(&mut chat_list).await
        })?;
        self.chat_list = chat_list;
        Ok(())
    }

    fn new_chat(&mut self) {
        debug!("创建新对话");
        let chat_count = self.chat_list.chats.len();
        let new_chat = Chat {
            id: Uuid::new_v4().to_string(),
            name: format!("新对话 {}", chat_count + 1),
            messages: Vec::new(),
            has_been_renamed: false,
        };
        let id = new_chat.id.clone();
        
        self.chat_list.chats.insert(0, new_chat);
        self.chat_list.current_chat_id = Some(id);
        self.chat_history.0.clear();
        self.input_focus = true;
        
        if let Err(e) = self.save_chat_list() {
            error!("保存聊天列表失败: {}", e);
        }
    }

    fn send_message(&mut self) {
        debug!("开始发送消息");
        let user_input = std::mem::take(&mut self.input_text);
        let image_path = self.selected_image.take();
        
        // 如果没有选中的聊天，创建一个新的
        if self.chat_list.current_chat_id.is_none() {
            debug!("没有选中的聊天，创建新对话");
            self.new_chat();
        }
        
        // 处理图片
        let processed_image = if let Some(processing) = self.processing_image.take() {
            match self.runtime_handle.block_on(async {
                match processing.await {
                    Ok(result) => result,
                    Err(_) => Err(ImageError::IoError(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "图片处理任务被取消"
                    )))
                }
            }) {
                Ok(path) => Some(path),
                Err(e) => {
                    error!("图片处理失败: {}", e);
                    None
                }
            }
        } else if let Some(ref path) = image_path {
            match self.runtime_handle.block_on(async {
                utils::copy_to_cache(path).await
            }) {
                Ok(path) => Some(path),
                Err(e) => {
                    error!("图片处理失败: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // 创建用户消息时使用处理后的图片路径
        let mut new_message = Message::new_user(
            user_input.clone(),
            processed_image.map(|p| p.to_string_lossy().to_string()),
        );
        
        debug!("检查是否需要生成标题");
        let should_generate_title = if let Some(current_id) = &self.chat_list.current_chat_id {
            self.chat_list.chats
                .iter()
                .find(|c| &c.id == current_id)
                .map(|chat| !chat.has_been_renamed && chat.messages.is_empty())
                .unwrap_or(false)
        } else {
            false
        };

        debug!("准备发消息，是否包含图片: {}", image_path.is_some());

        // 创建通道
        let (tx, rx) = mpsc::unbounded_channel();
        self.receiver = Some(rx);

        // 立即创建并添加用户消息
        self.chat_history.add_message(new_message.clone());

        // 启动异步任务
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let api_endpoint = self.api_endpoint.clone();
        let model_name = self.model_name.clone();
        let temperature = self.temperature;
        let system_prompt = self.system_prompt.clone();
        let retry_enabled = self.retry_enabled;
        let max_retries = self.max_retries;
        let history_messages = self.chat_history.0.clone();
        let chat_id = self.chat_list.current_chat_id.clone();
        let should_generate_title = should_generate_title;
        let tx_clone = tx.clone();  // 克隆通道发送端

        self.runtime.spawn(async move {
            // 先处理图片（如果有）
            let cached_image_path = if let Some(path) = image_path {
                // 如果已经有处理过的图片路径，直接使用它
                if let Some(ref processed_path) = new_message.image_path {
                    debug!("使用已处理的缓存图片: {:?}", processed_path);
                    Some(PathBuf::from(processed_path))
                } else {
                    // 否则才进行处理
                    match utils::copy_to_cache(&path).await {
                        Ok(cache_path) => {
                            debug!("图片已复制到缓存: {:?}", cache_path);
                            Some(cache_path)
                        }
                        Err(e) => {
                            error!("处理图片失败: {}", e);
                            None
                        }
                    }
                }
            } else {
                None
            };

            // 更新消息中的图片路径（如果还没有设置的话）
            if new_message.image_path.is_none() {
                if let Some(path) = cached_image_path.clone() {
                    new_message.image_path = Some(path.to_string_lossy().to_string());
                    // 发送消息更通知
                    let _ = tx_clone.send(format!("__UPDATE_MESSAGE_IMAGE__:{}", path.to_string_lossy()));
                }
            }

            // 构建消息数组
            let mut messages = vec![
                json!({
                    "role": "system",
                    "content": system_prompt
                })
            ];

            // 添加历史消息
            for msg in history_messages {
                if let Ok(content) = msg.to_api_content().await {
                    messages.push(json!({
                        "role": msg.role,
                        "content": content
                    }));
                }
            }

            // 添加新消息（包含处理后的图片）
            if let Ok(content) = new_message.to_api_content().await {
                messages.push(json!({
                    "role": "user",
                    "content": content
                }));
            }

            // 发送请求
            let payload = json!({
                "model": model_name,
                "messages": messages,
                "temperature": temperature,
                "stream": true
            });

            // 发送请求
            if let Err(e) = api::send_request(
                &client,
                &api_endpoint,
                &api_key,
                &payload,
                retry_enabled,
                max_retries,
                &tx_clone
            ).await {
                error!("发送请求失败: {:?}", e);
                let _ = tx_clone.send(format!("错误: {}", e));
                let _ = tx_clone.send("__STREAM_DONE__".to_string());
            }

            // 如果需要生成标题
            if should_generate_title {
                debug!("需要生成标题，当前对话ID: {:?}", chat_id);
                debug!("开始生成标题，用户输入: {}", user_input);
                let title_payload = json!({
                    "model": model_name.clone(),
                    "messages": vec![
                        json!({
                            "role": "system",
                            "content": "请根据用户的输生成一个简的标题(不超过20个字),直接返回标题即可,不需要任何解释或额外的标点符号。"
                        }),
                        json!({
                            "role": "user",
                            "content": user_input.clone()
                        }),
                    ],
                    "temperature": 0.7,
                    "max_tokens": 60
                });

                debug!("发送标题生成请求: {}", title_payload);
                // 发送标题生成请求
                match client
                    .post(&api_endpoint)
                    .header("Authorization", format!("Bearer {}", api_key))
                    .header("Content-Type", "application/json")
                    .json(&title_payload)
                    .send()
                    .await
                {
                    Ok(response) => {
                        debug!("收到标题生成响应: {:?}", response.status());
                        match response.json::<JsonValue>().await {
                            Ok(json) => {
                                debug!("标题生成响应JSON: {:?}", json);
                                if let Some(title) = json["choices"][0]["message"]["content"]
                                    .as_str()
                                    .map(|s| s.trim().to_string())
                                {
                                    debug!("成功生成标题: {}", title);
                                    if let Some(chat_id) = chat_id {
                                        let title_message = format!("__TITLE_UPDATE__{}:{}", chat_id, title);
                                        debug!("发送标题更新消息: {}", title_message);
                                        if let Err(e) = tx_clone.send(title_message) {
                                            error!("发送标题更新消息失败: {}", e);
                                        }
                                    } else {
                                        debug!("没有找到对话ID，无法更新标题");
                                    }
                                } else {
                                    error!("无法从响应中提取标题");
                                }
                            }
                            Err(e) => {
                                error!("析标题生成响应失败: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("标题生成请求失败: {}", e);
                    }
                }
            } else {
                debug!("不需要生成标题");
            }
        });
    }

    fn handle_message_selection(&mut self, messages: Vec<Message>) {
        debug!("选择消息: {} 条", messages.len());
        self.chat_history.0 = messages;
    }

    fn handle_response(&mut self, response: String) {
        debug!("处理响应: {}", response);
        if self.chat_history.last_message_is_assistant() {
            if let Some(last_msg) = self.chat_history.0.last_mut() {
                last_msg.content.push_str(&response);
            }
        } else {
            debug!("加新的助手消息");
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

    async fn load_image_async(&self, path: &str) -> Option<(u32, u32, Vec<u8>)> {
        debug!("异步加载图片: {}", path);
        // 异步读取图片文件
        let image_bytes = match tokio::fs::read(path).await {
            Ok(bytes) => {
                debug!("读取图片文件成功，大小: {} bytes", bytes.len());
                bytes
            }
            Err(e) => {
                error!("读取图片文件失败: {}", e);
                return None;
            }
        };

        // 在单独的线程���处理片
        let result = tokio::task::spawn_blocking(move || {
            let image = match image::load_from_memory(&image_bytes) {
                Ok(img) => img,
                Err(e) => {
                    error!("加载图片失败: {}", e);
                    return None;
                }
            };

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
            
            Some((width, height, resized.as_raw().to_vec()))
        }).await.unwrap_or(None);

        result
    }

    fn load_image(&mut self, ui: &mut egui::Ui, path: &str) -> Option<egui::TextureHandle> {
        debug!("加载图片: {}", path);
        // 使用 block_on 执行步加载
        if let Some((width, height, pixels)) = self.runtime_handle.block_on(async {
            self.load_image_async(path).await
        }) {
            debug!("图片加载成功: {}x{}", width, height);
            let texture = ui.ctx().load_texture(
                format!("img_{}", path.replace("/", "_")),
                egui::ColorImage::from_rgba_unmultiplied(
                    [width as _, height as _],
                    &pixels
                ),
                egui::TextureOptions::default(),
            );
            
            return Some(texture);
        }
        debug!("图片加载失败");
        None
    }
}

impl Clone for ChatApp {
    fn clone(&self) -> Self {
        Self {
            input_text: self.input_text.clone(),
            chat_history: self.chat_history.clone(),
            api_key: self.api_key.clone(),
            runtime: Runtime::new().unwrap(),
            runtime_handle: self.runtime.handle().clone(),
            receiver: None,
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
            processing_image: None,
            dark_mode: self.dark_mode,
            available_models: self.available_models.clone(),
            input_focus: self.input_focus,
        }
    }
}

impl eframe::App for ChatApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // 如果正在接收消息流，设置较高的刷新率
        if self.receiver.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        
        // 在每次更新时设置主题
        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

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
                                    
                                    // 创建一个反迭代器来倒序显聊天列表
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
                                ui.horizontal(|ui| {
                                    if ui.button("⚙").clicked() {
                                        self.show_settings = !self.show_settings;
                                    }
                                    
                                    // 添主题切换按钮
                                    if ui.button(if self.dark_mode { "☀" } else { "🌙" }).clicked() {
                                        self.dark_mode = !self.dark_mode;
                                        // 保存主题设置
                                        if let Err(e) = self.save_config(frame) {
                                            error!("保存配置失败: {}", e);
                                        }
                                    }
                                });
                            });
                        });
                    });

                // 修改删除快捷键检查的部分
                if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Backspace)) {
                    if let Some(current_id) = self.chat_list.current_chat_id.clone() {
                        debug!("开始删除对话: {}", current_id);
                        
                        // 获取要删除的对话
                        if let Some(chat) = self.chat_list.chats.iter().find(|c| c.id == current_id) {
                            debug!("找到要删除的对话: {} ({})", chat.name, chat.id);
                            // 删除所有相关的缓存图片
                            let messages = chat.messages.clone();
                            let runtime_handle = self.runtime_handle.clone();
                            debug!("开始清理对话中的图片缓存，消息数量: {}", messages.len());
                            
                            runtime_handle.spawn(async move {
                                for (index, msg) in messages.iter().enumerate() {
                                    if let Some(image_path) = &msg.image_path {
                                        debug!("处理第 {} 条消息的图片: {}", index + 1, image_path);
                                        if let Err(e) = utils::remove_cached_image(image_path).await {
                                            error!("删除第 {} 条消息的缓存图片失败: {} - {}", 
                                                index + 1, image_path, e);
                                        }
                                    }
                                }
                                debug!("图片缓存清理完成");
                            });
                        } else {
                            debug!("未找到要删除的对话: {}", current_id);
                        }
                        
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
                        
                        debug!("对话删除完成");
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
                                    ui.label(&self.model_name);  // 将输入框改为只读标签
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

                                    // 添加模型管理部分
                                    ui.label("常用模型:");
                                    ui.vertical(|ui| {
                                        // 显示现有模型列表
                                        let mut models_to_remove = Vec::new();
                                        for (index, model) in self.available_models.iter().enumerate() {
                                            ui.horizontal(|ui| {
                                                ui.label(model);
                                                if ui.button("🗑").clicked() {
                                                    models_to_remove.push(index);
                                                    config_changed = true;
                                                }
                                            });
                                        }
                                        
                                        // 删除标记的模型
                                        for index in models_to_remove.iter().rev() {
                                            self.available_models.remove(*index);
                                        }

                                        // 添加新模型的输入框
                                        static mut NEW_MODEL: String = String::new();
                                        unsafe {
                                            ui.horizontal(|ui| {
                                                let text_edit = ui.text_edit_singleline(&mut NEW_MODEL);
                                                if ui.button("添加").clicked() && !NEW_MODEL.is_empty() {
                                                    if !self.available_models.contains(&NEW_MODEL) {
                                                        self.available_models.push(NEW_MODEL.clone());
                                                        NEW_MODEL.clear();
                                                        config_changed = true;
                                                    }
                                                }
                                            });
                                        }
                                    });
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
                    // 当设置面关闭时打印日志
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
                        // 图片上传按钮、文件名显示和模型选择放在上方
                        ui.horizontal(|ui| {
                            if ui.button("📎").clicked() {
                                if let Some(path) = FileDialog::new()
                                    .add_filter("图片", &["png", "jpg", "jpeg"])
                                    .pick_file() 
                                {
                                    self.selected_image = Some(path.clone());
                                    // 立即开始处理图片
                                    let runtime_handle = self.runtime_handle.clone();
                                    self.processing_image = Some(runtime_handle.spawn(async move {
                                        utils::copy_to_cache(&path).await
                                    }));
                                }
                            }
                            
                            // 显示图片文件名和删除按钮
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

                            // 修改模型选择部分，使用图标
                            ui.add_space(10.0);
                            egui::ComboBox::from_id_source("model_selector")
                                .selected_text(&self.model_name)
                                .show_ui(ui, |ui| {
                                    for model in &self.available_models {
                                        if ui.selectable_value(&mut self.model_name, model.clone(), model).changed() {
                                            if let Err(e) = self.save_config(frame) {
                                                error!("保存配置失败: {}", e);
                                            }
                                        }
                                    }
                                })
                                .response
                                .on_hover_text("选择模型");
                        });

                        // 输入框和发送按钮在下方
                        ui.horizontal(|ui| {
                            let text_edit = TextEdit::multiline(&mut self.input_text)
                                .desired_rows(3)
                                .min_size(egui::vec2(available_width - 50.0, 60.0))
                                .id("chat_input".into());
                            
                            let text_edit_response = ui.add(text_edit);
                            
                            // 如果需要聚焦且输入框还没有焦点
                            if self.input_focus && !text_edit_response.has_focus() {
                                text_edit_response.request_focus();
                            }
                            // 一旦获得焦点，就将 input_focus 设置为 false
                            if text_edit_response.has_focus() {
                                self.input_focus = false;
                            }
                            
                            if ui.add_sized(
                                [40.0, 60.0],
                                egui::Button::new("➤")
                            ).clicked() || (ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
                                && text_edit_response.has_focus())
                            {
                                if !self.input_text.is_empty() || self.selected_image.is_some() {
                                    self.send_message();
                                    self.input_focus = true;  // 发送消息后重新设置焦点标志
                                }
                            }
                        });
                    });
                });
            });

            // 处理消息接收器 - 每帧最多处理一条消息
            if let Some(receiver) = &mut self.receiver {
                if let Ok(response) = receiver.try_recv() {  // 只获取一条消息
                    match response.as_str() {
                        s if s.starts_with("__UPDATE_MESSAGE_IMAGE__:") => {
                            if let Some(path) = s.strip_prefix("__UPDATE_MESSAGE_IMAGE__:") {
                                if let Some(last_msg) = self.chat_history.0.last_mut() {
                                    last_msg.image_path = Some(path.to_string());
                                }
                            }
                        }
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
                                        chat.messages = self.chat_history.0.clone();  // 同时更新消息历史
                                        
                                        // 保存更新后的聊天列表
                                        if let Err(e) = self.save_chat_list() {
                                            error!("保存聊天列表失败: {}", e);
                                        }
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
                                    if let Err(e) = self.save_chat_list() {
                                        error!("保存聊天列表失败: {}", e);
                                    }
                                }
                            }
                        }
                        response => {
                            self.handle_response(response.to_string());
                        }
                    }
                }
            }
        });
    }
} 