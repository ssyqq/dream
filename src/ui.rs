use crate::api;
use crate::config;
use crate::models::{Chat, ChatConfig, ChatHistory, ChatList, Message};
use crate::utils::{self, ImageError};
use chrono::Utc;
use eframe::egui::{self, RichText, ScrollArea, TextEdit};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use log::{debug, error};
use reqwest::Client;
use rfd::FileDialog;
use serde_json::{json, Value as JsonValue};
use std::path::PathBuf;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use uuid::Uuid;

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
    pub processing_image: Option<tokio::task::JoinHandle<Result<PathBuf, ImageError>>>,
    pub dark_mode: bool,
    pub available_models: Vec<String>,
    pub input_focus: bool,
    pub markdown_cache: CommonMarkCache,
    pub new_model_input: String,
    pub show_role_creator: bool,
    pub role_name_input: String,
    pub role_prompt_input: String,
    pub role_model_name: String,
    pub role_temperature: f32,
    pub clear_chat_mode: bool,
    pub input_height: f32,
    pub dragging_input: bool,
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
        let config = runtime_handle.block_on(async { config::load_config().await });

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
            processing_image: None,
            dark_mode: config.chat.dark_mode,
            available_models: config.api.available_models,
            input_focus: true,
            markdown_cache: CommonMarkCache::default(),
            new_model_input: String::new(),
            show_role_creator: false,
            role_name_input: String::new(),
            role_prompt_input: String::new(),
            role_model_name: "gpt-4".to_string(),
            role_temperature: 0.7,
            clear_chat_mode: true,
            input_height: 120.0,
            dragging_input: false,
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
                config: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
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
        debug!("创建新的 ChatApp 实");
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
        let config = handle.block_on(async { config::load_config().await });
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
            processing_image: None,
            dark_mode: config.chat.dark_mode,
            available_models: config.api.available_models,
            input_focus: true,
            markdown_cache: CommonMarkCache::default(),
            new_model_input: String::new(),
            show_role_creator: false,
            role_name_input: String::new(),
            role_prompt_input: String::new(),
            role_model_name: "gpt-4".to_string(),
            role_temperature: 0.7,
            clear_chat_mode: true,
            input_height: 120.0,
            dragging_input: false,
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
                config: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
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
        self.runtime_handle
            .block_on(async { config::save_config(&config).await })
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn save_chat_list_async(&self) -> Result<(), Box<dyn std::error::Error>> {
        debug!("正在保存聊天列表...");
        // 在保存之前先克隆并反转列表，这样保存的顺序就和加载时的顺序一致
        let mut save_list = self.chat_list.clone();
        save_list.chats.reverse();
        let json = serde_json::to_string_pretty(&save_list)?;
        tokio::fs::write("chat_list.json", json).await?;
        debug!("聊天列表保存成功");
        Ok(())
    }

    fn save_chat_list(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.runtime_handle
            .block_on(async { self.save_chat_list_async().await })
    }

    async fn load_chat_list_async(
        chat_list: &mut ChatList,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(content) = tokio::fs::read_to_string("chat_list.json").await {
            *chat_list = serde_json::from_str(&content)?;
            // 加载后反转列表顺序，使其与显示顺序一致
            chat_list.chats.reverse();
        }
        Ok(())
    }

    fn load_chat_list(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut chat_list = self.chat_list.clone();
        self.runtime_handle
            .block_on(async { Self::load_chat_list_async(&mut chat_list).await })?;
        self.chat_list = chat_list;
        Ok(())
    }

    fn new_chat(&mut self) {
        debug!("创建新对话");
        let chat_count = self.chat_list.chats.len();
        let name = format!("新对话 {}", chat_count + 1);

        // 使用 new 方法创建新对话，它会自动设置 config 为 None
        let new_chat = Chat::new(name);
        let id = new_chat.id.clone();

        // 将新对话添加到列表开头，而不是末尾
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

        // 更新当前聊天的时间戳
        if let Some(current_id) = &self.chat_list.current_chat_id {
            if let Some(chat) = self
                .chat_list
                .chats
                .iter_mut()
                .find(|c| &c.id == current_id)
            {
                chat.update_time();
            }
        }

        // 获取当前聊天的配置
        let (current_model, current_prompt, current_temp) =
            if let Some(current_id) = &self.chat_list.current_chat_id {
                if let Some(chat) = self.chat_list.chats.iter().find(|c| &c.id == current_id) {
                    if let Some(config) = &chat.config {
                        (
                            config.model_name.clone(),
                            config.system_prompt.clone(),
                            config.temperature,
                        )
                    } else {
                        (
                            self.model_name.clone(),
                            self.system_prompt.clone(),
                            self.temperature,
                        )
                    }
                } else {
                    (
                        self.model_name.clone(),
                        self.system_prompt.clone(),
                        self.temperature,
                    )
                }
            } else {
                (
                    self.model_name.clone(),
                    self.system_prompt.clone(),
                    self.temperature,
                )
            };

        // 处理图片
        let processed_image = if let Some(processing) = self.processing_image.take() {
            match self.runtime_handle.block_on(async {
                match processing.await {
                    Ok(result) => result,
                    Err(_) => Err(ImageError::IoError(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "图片处理任务被取消",
                    ))),
                }
            }) {
                Ok(path) => Some(path),
                Err(e) => {
                    error!("图片处理失败: {}", e);
                    None
                }
            }
        } else if let Some(ref path) = image_path {
            match self
                .runtime_handle
                .block_on(async { utils::copy_to_cache(path).await })
            {
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
            self.chat_list
                .chats
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
        let tx_clone = tx.clone(); // 克隆通道发送端

        // 在 spawn 之前克隆需要的数据
        let chat_history = self.chat_history.0.clone();

        self.runtime.spawn(async move {
            // 先处理图片（如果有）
            let cached_image_path = if let Some(path) = image_path {
                // 如果已经有理的图片路径，直接使用它
                if let Some(ref processed_path) = new_message.image_path {
                    debug!("使用已处理的缓图片: {:?}", processed_path);
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
                    // 送消息更通
                    let _ = tx_clone.send(format!("__UPDATE_MESSAGE_IMAGE__:{}", path.to_string_lossy()));
                }
            }

            // 构建消息数组时使用当前配置
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

            // 发送请求时使用当前配置
            let payload = json!({
                "model": current_model,
                "messages": messages,
                "temperature": current_temp,
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

            // 在等待助手回复完成后再生成标题
            if should_generate_title {
                debug!("需要生成标题，当前对话ID: {:?}", chat_id);
                // 使用克隆的 chat_history 而不是 self.chat_history
                let assistant_response = chat_history.last()
                    .filter(|msg| msg.role == "assistant")
                    .map(|msg| msg.content.clone())
                    .unwrap_or_default();

                debug!("开始生成标题，用户输入: {}", user_input);
                debug!("助手回复: {}", assistant_response);

                let title_payload = json!({
                    "model": model_name.clone(),
                    "messages": vec![
                        json!({
                            "role": "system",
                            "content": "请根据用户的输入AI的回复生成一个简短的对话标题(不超过20个字),直接返回标题即可,不需要任何解释或额外的标点符号。标题应该概括对话的主要内容或主题。"
                        }),
                        json!({
                            "role": "user",
                            "content": user_input
                        }),
                        json!({
                            "role": "assistant",
                            "content": assistant_response
                        })
                    ],
                    "temperature": 0.7,
                    "max_tokens": 60
                });

                debug!("发送标题生成请求: {}", title_payload);
                // 发送标题生成请
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
                                error!("解析标题生成响应失败: {}", e);
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

        // 不再在这里修改全局配置
        // 只需要加载消息历史即可
        // 发送消息时会自动使用角色的配置
    }

    fn handle_response(&mut self, response: String) {
        debug!("处理响应: {}", response);
        if self.chat_history.last_message_is_assistant() {
            if let Some(last_msg) = self.chat_history.0.last_mut() {
                last_msg.content.push_str(&response);
            }
        } else {
            debug!("加新的助手消息");
            self.chat_history
                .add_message(Message::new_assistant(response));
        }
    }

    fn display_message(&mut self, ui: &mut egui::Ui, msg: &Message) {
        match msg.role.as_str() {
            "user" => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("You:").strong().size(16.0));
                    ui.add_space(8.0);
                });
                ui.add_space(4.0);

                // 构建包含图片的 markdown 内容
                let content = if let Some(path) = &msg.image_path {
                    // 直接使用 markdown 图片语法
                    format!("{}\n\n![image]({})", msg.content, path)
                } else {
                    msg.content.clone()
                };

                // 使用 CommonMarkViewer 渲染完整内容
                ui.ctx().set_theme(egui::Theme::Light);
                let viewer = if self.dark_mode {
                    CommonMarkViewer::new().syntax_theme_dark("fuck")
                } else {
                    CommonMarkViewer::new().syntax_theme_light("fuck")
                };
                viewer.show(ui, &mut self.markdown_cache, &content);
            }
            "assistant" => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("AI:").strong().size(16.0));
                    ui.add_space(8.0);
                });
                ui.add_space(4.0);

                let viewer = if self.dark_mode {
                    CommonMarkViewer::new().syntax_theme_dark("fuck")
                } else {
                    CommonMarkViewer::new().syntax_theme_light("fuck")
                };
                viewer.show(ui, &mut self.markdown_cache, &msg.content);
            }
            _ => {}
        }
    }

    // 添加创建角色的函数
    fn create_role(&mut self) {
        let new_chat = Chat {
            id: Uuid::new_v4().to_string(),
            name: format!("\u{f544} {}", self.role_name_input.trim()),
            messages: Vec::new(),
            has_been_renamed: true,
            config: Some(ChatConfig {
                model_name: self.role_model_name.clone(),
                system_prompt: self.role_prompt_input.clone(),
                temperature: self.role_temperature,
            }),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        // 将角色添加到列表最前面
        self.chat_list.chats.insert(0, new_chat);

        // 保存聊天列表
        if let Err(e) = self.save_chat_list() {
            error!("保存聊天列表失败: {}", e);
        }

        // 清空输入
        self.role_name_input.clear();
        self.role_prompt_input.clear();
        self.role_temperature = 0.7;
        self.show_role_creator = false;
    }

    // 修改清空聊天的处理逻辑
    fn clear_chat(&mut self, chat_id: &str) {
        if self.clear_chat_mode {
            // 完全清空模式：清空内存和保存的记录
            self.chat_history.0.clear();
            if let Some(chat) = self.chat_list.chats.iter_mut().find(|c| &c.id == chat_id) {
                chat.messages.clear();
                // 保存更新后的聊天列表
                if let Err(e) = self.save_chat_list() {
                    error!("保存聊天列表失败: {}", e);
                }
            }
        } else {
            // 仅清空内存模式：添加分隔线消息
            self.chat_history.add_message(Message::new_assistant(
                "--------------------------- 历史记录分割线 ---------------------------"
                    .to_string(),
            ));
        }
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
            processing_image: None,
            dark_mode: self.dark_mode,
            available_models: self.available_models.clone(),
            input_focus: self.input_focus,
            markdown_cache: CommonMarkCache::default(),
            new_model_input: self.new_model_input.clone(),
            show_role_creator: self.show_role_creator,
            role_name_input: self.role_name_input.clone(),
            role_prompt_input: self.role_prompt_input.clone(),
            role_model_name: self.role_model_name.clone(),
            role_temperature: self.role_temperature,
            clear_chat_mode: self.clear_chat_mode,
            input_height: self.input_height,
            dragging_input: self.dragging_input,
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
                            ui.add_space(5.0);
                            ui.horizontal(|ui| {
                                if ui.small_button("\u{f067}").clicked() {
                                    self.new_chat();
                                }
                            });

                            ui.separator();

                            // 聊天列表区域 - 设置为充满剩余空间
                            ScrollArea::vertical()
                                .auto_shrink([false; 2])
                                .drag_to_scroll(false)
                                .show(ui, |ui| {
                                    let mut selected_messages = None;
                                    let mut selected_id = None;

                                    // 分别获取角色聊天和普通聊天
                                    let (mut role_chats, mut normal_chats): (Vec<_>, Vec<_>) = self
                                        .chat_list
                                        .chats
                                        .iter()
                                        .partition(|chat| chat.name.starts_with("\u{f544}"));

                                    // 对普通聊天按更新时间排序（新的在前）
                                    normal_chats.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));

                                    // 对角色聊天按更新时间排序（新的在前）
                                    role_chats.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

                                    // 显示角色聊天
                                    for chat in &role_chats {
                                        let is_selected = self
                                            .chat_list
                                            .current_chat_id
                                            .as_ref()
                                            .map_or(false, |id| id == &chat.id);

                                        ui.horizontal(|ui| {
                                            ui.set_min_height(24.0);

                                            let response = ui.selectable_label(
                                                is_selected,
                                                RichText::new(&chat.name),
                                            );

                                            if response.clicked() {
                                                selected_id = Some(chat.id.clone());
                                                selected_messages = Some(chat.messages.clone());
                                            }
                                        });
                                    }

                                    // 添加分割线
                                    if !role_chats.is_empty() && !normal_chats.is_empty() {
                                        ui.add_space(4.0);
                                        ui.separator();
                                        ui.add_space(4.0);
                                    }

                                    // 显示普通聊天（反转顺序）
                                    for chat in normal_chats.iter().rev() {
                                        // 这里添加 .rev()
                                        let is_selected = self
                                            .chat_list
                                            .current_chat_id
                                            .as_ref()
                                            .map_or(false, |id| id == &chat.id);

                                        ui.horizontal(|ui| {
                                            ui.set_min_height(24.0);

                                            let response = ui.selectable_label(
                                                is_selected,
                                                RichText::new(&chat.name),
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
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    if ui.small_button("\u{f013}").clicked() {
                                        // nf-fa-cog 设置按钮
                                        self.show_settings = !self.show_settings;
                                    }

                                    if ui.small_button("\u{f007}").clicked() {
                                        // nf-fa-user 角色按钮
                                        self.show_role_creator = !self.show_role_creator;
                                    }

                                    // 主题切换按钮
                                    if ui
                                        .small_button(if self.dark_mode {
                                            "\u{f185}" // nf-fa-sun_o
                                        } else {
                                            "\u{f186}" // nf-fa-moon_o
                                        })
                                        .clicked()
                                    {
                                        self.dark_mode = !self.dark_mode;
                                        if let Err(e) = self.save_config(frame) {
                                            error!("保存配置失败: {}", e);
                                        }
                                    }
                                });
                                ui.separator(); // 在按钮上方添加分割线
                            });
                        });
                    });

                // 修改删除快捷键检查的部分
                if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Backspace)) {
                    if let Some(current_id) = self.chat_list.current_chat_id.clone() {
                        debug!("开始删除对话: {}", current_id);

                        // 获取要删除的对话
                        if let Some(chat) = self.chat_list.chats.iter().find(|c| c.id == current_id)
                        {
                            debug!("找到要删除的对话: {} ({})", chat.name, chat.id);
                            // 删除所有相关的缓存图片
                            let messages = chat.messages.clone();
                            let runtime_handle = self.runtime_handle.clone();
                            debug!("开始清理对话的图片缓存，消数: {}", messages.len());

                            runtime_handle.spawn(async move {
                                for (index, msg) in messages.iter().enumerate() {
                                    if let Some(image_path) = &msg.image_path {
                                        debug!("处理第 {} 条消息的图片: {}", index + 1, image_path);
                                        if let Err(e) = utils::remove_cached_image(image_path).await
                                        {
                                            error!(
                                                "删除第 {} 条消的缓存图片失败: {} - {}",
                                                index + 1,
                                                image_path,
                                                e
                                            );
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
            let history_height = total_height - self.input_height;

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

                                    // 默认模型设置 - 改为下拉选择
                                    ui.label("默认模型:");
                                    egui::ComboBox::from_id_salt("default_model_selector")
                                        .selected_text(&self.model_name)
                                        .width(ui.available_width() - 60.0)
                                        .show_ui(ui, |ui| {
                                            for model in &self.available_models {
                                                if ui.selectable_value(&mut self.model_name, model.clone(), model).changed() {
                                                    config_changed = true;
                                                }
                                            }
                                        });
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
                                    ui.label("常用模:");
                                    ui.vertical(|ui| {
                                        // 显示现有模型列表
                                        let mut models_to_remove = Vec::new();
                                        for (index, model) in self.available_models.iter().enumerate() {
                                            ui.horizontal(|ui| {
                                                ui.label(model);
                                                if ui.small_button("\u{f1f8}").clicked() {
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
                                        ui.horizontal(|ui| {
                                            if ui.text_edit_singleline(&mut self.new_model_input).lost_focus()
                                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                                && !self.new_model_input.is_empty()
                                            {
                                                if !self.available_models.contains(&self.new_model_input) {
                                                    self.available_models.push(self.new_model_input.clone());
                                                    self.new_model_input.clear();
                                                    config_changed = true;
                                                }
                                            }
                                            if ui.small_button("添加").clicked() && !self.new_model_input.is_empty() {
                                                if !self.available_models.contains(&self.new_model_input) {
                                                    self.available_models.push(self.new_model_input.clone());
                                                    self.new_model_input.clear();
                                                    config_changed = true;
                                                }
                                            }
                                        });
                                    });
                                    ui.end_row();

                                    // 添加聊天记录清空模式设置
                                    ui.label("清空聊天模式:");
                                    ui.horizontal(|ui| {
                                        if ui.radio(self.clear_chat_mode, "完全清空").clicked() {
                                            self.clear_chat_mode = true;
                                        }
                                        if ui.radio(!self.clear_chat_mode, "仅清空内存").clicked() {
                                            self.clear_chat_mode = false;
                                        }
                                    });
                                    ui.end_row();
                                });

                            if config_changed {
                                debug!("配置已更改正在保存");
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
                            if i > 0 && i % 2 == 0 {
                                ui.add_space(4.0);
                                ui.separator();
                                ui.add_space(4.0);
                            }
                            self.display_message(ui, msg);
                        }
                    });

                // 修改分隔条的部分
                let separator_height = 0.0;
                let full_width = ui.available_width() + 20.0;
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(full_width, separator_height),
                    egui::Sense::drag(),
                );

                // 绘制分隔条
                if ui.is_rect_visible(rect) {
                    // 创建向左偏移的矩形
                    let adjusted_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.min.x - 10.0, rect.min.y),
                        egui::vec2(full_width, separator_height)
                    );
                    
                    // 基础线条
                    let base_stroke = ui.style().visuals.widgets.noninteractive.bg_stroke;
                    ui.painter().rect(
                        adjusted_rect,
                        0.0,
                        ui.style().visuals.window_fill(),
                        base_stroke,
                    );

                    // hover 效果
                    if response.hovered() || response.dragged() {
                        let hover_stroke = ui.style().visuals.selection.stroke;
                        let hover_stroke = egui::Stroke::new(2.0, hover_stroke.color);
                        ui.painter().line_segment(
                            [
                                egui::pos2(rect.left() - 10.0, rect.center().y),
                                egui::pos2(rect.right(), rect.center().y)
                            ],
                            hover_stroke,
                        );
                    }
                }

                // 处理拖动逻辑
                if response.hovered() {
                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeVertical);
                }

                // 使用拖动增量来调整高度
                let delta = response.drag_delta();  // drag_delta() 直接返回 Vec2
                self.input_height = (self.input_height - delta.y)
                    .clamp(80.0, total_height * 0.8);

                // 添加视觉反馈
                if response.hovered() || response.dragged() {
                    let stroke = ui.style().visuals.selection.stroke;
                    let stroke = egui::Stroke::new(2.0, stroke.color);  // 创建新的 Stroke 来设置宽度
                    ui.painter().line_segment(
                        [rect.left_center(), rect.right_center()],
                        stroke,
                    );
                }

                // 输入区域
                ui.horizontal(|ui| {
                    let available_width = ui.available_width();

                    // 修改输入区域的布局
                    ui.vertical(|ui| {
                        // 图片上传按钮和文件名显示
                        ui.horizontal(|ui| {
                            if ui.small_button("\u{f0c6}").clicked() {
                                if let Some(path) = FileDialog::new()
                                    .add_filter("图片", &["png", "jpg", "jpeg"])
                                    .pick_file()
                                {
                                    self.selected_image = Some(path.clone());
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
                                        if ui.small_button("\u{f00d}").clicked() {
                                            should_clear_image = true;
                                        }
                                    }
                                }
                            }
                            if should_clear_image {
                                self.selected_image = None;
                            }
                        });

                        // 将输入框放在 ScrollArea 中，使用动态高度
                        ScrollArea::vertical()
                            .min_scrolled_height(self.input_height - 40.0) // 减去按钮和间距的高度
                            .show(ui, |ui| {
                                let text_edit = TextEdit::multiline(&mut self.input_text)
                                    .desired_width(available_width) // 减小宽度以适应滚动条
                                    .desired_rows(4)  // 减少默认行数
                                    .frame(false);

                                let text_edit_response = ui.add(text_edit);

                                // 如果需要聚焦且输入框还没有焦点
                                if self.input_focus && !text_edit_response.has_focus() {
                                    text_edit_response.request_focus();
                                }
                                // 一旦获得焦点，将 input_focus 设置为 false
                                if text_edit_response.has_focus() {
                                    self.input_focus = false;
                                }

                                // 检查 Enter 键发送
                                if (ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
                                    && text_edit_response.has_focus())
                                    && (!self.input_text.is_empty() || self.selected_image.is_some())
                                {
                                    self.send_message();
                                    self.input_focus = true;
                                }
                            });

                        ui.add_space(4.0); // 添加一点底部间距
                    });

                    // 将清空按钮移到右侧
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        // 只在角色聊天中显示清空按钮
                        let should_clear = if let Some(current_id) = &self.chat_list.current_chat_id {
                            if let Some(chat) = self.chat_list.chats.iter().find(|c| &c.id == current_id) {
                                chat.name.starts_with("\u{f544}")
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if should_clear {
                            if ui.button("\u{f51a}").clicked() {
                                if let Some(id) = self.chat_list.current_chat_id.clone() {
                                    self.clear_chat(&id);
                                }
                            }
                        }
                    });
                });
            });

            // 处理消息接收器 - 每帧最多处理一条消息
            if let Some(receiver) = &mut self.receiver {
                if let Ok(response) = receiver.try_recv() {  // 只获取一条消息
                    match response.as_str() {
                        "__CLEAR_ERRORS__" => {
                            // 清空最后一条消息如果它是错误提示
                            if let Some(last_msg) = self.chat_history.0.last() {
                                if last_msg.content.starts_with("遇到") {
                                    self.chat_history.0.pop();
                                }
                            }
                        }
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
                                        chat.messages = self.chat_history.0.clone();  // 同更新消息历史

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

                                    // 在这里生成标题
                                    if !chat.has_been_renamed {
                                        debug!("开始生成标题");
                                        // 获取用户输入和完整的助手回复
                                        let user_input = chat.messages.iter()
                                            .find(|msg| msg.role == "user")
                                            .map(|msg| msg.content.clone())
                                            .unwrap_or_default();

                                        let assistant_response = chat.messages.iter()
                                            .find(|msg| msg.role == "assistant")
                                            .map(|msg| msg.content.clone())
                                            .unwrap_or_default();

                                        let title_payload = json!({
                                            "model": self.model_name.clone(),
                                            "messages": vec![
                                                json!({
                                                    "role": "system",
                                                    "content": "你善于总结标题，标题不超过10个字，不要包含有任何解释和符号。"
                                                }),
                                                json!({
                                                    "role": "user",
                                                    "content": user_input
                                                }),
                                                json!({
                                                    "role": "assistant",
                                                    "content": assistant_response
                                                }),
                                                json!({
                                                    "role": "user",
                                                    "content": "总结我们对话的标题，标题不超过10个字，不要包含有任何解释和符号。"
                                                }),
                                            ],
                                            "temperature": 0.7,
                                            "max_tokens": 60
                                        });

                                        // 发送标题生成请求
                                        let runtime_handle = self.runtime_handle.clone();
                                        let api_endpoint = self.api_endpoint.clone();
                                        let api_key = self.api_key.clone();
                                        let chat_id = current_id.clone();
                                        let client = self.client.clone();

                                        // 创建新的通道用于标题更新
                                        let (tx, rx) = mpsc::unbounded_channel();

                                        runtime_handle.spawn(async move {
                                            debug!("发送标题生成请求: {}", title_payload);
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
                                                                let title_message = format!("__TITLE_UPDATE__{}:{}", chat_id, title);
                                                                debug!("发送标题更新消息: {}", title_message);
                                                                if let Err(e) = tx.send(title_message) {
                                                                    error!("发送标题更新消息失败: {}", e);
                                                                }
                                                            } else {
                                                                error!("无法从响应中提取标题");
                                                            }
                                                        }
                                                        Err(e) => {
                                                            error!("解析标题生成响应失败: {}", e);
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    error!("标题生成请求失败: {}", e);
                                                }
                                            }
                                        });

                                        // 设置新的接收器
                                        self.receiver = Some(rx);
                                    }

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

        // 添加角色创建窗口
        if self.show_role_creator {
            egui::Window::new("创建角色")
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.label("角色名称:");
                        ui.text_edit_singleline(&mut self.role_name_input);

                        ui.add_space(8.0);
                        ui.label("选择模型:");
                        egui::ComboBox::from_id_salt("role_model_selector")
                            .selected_text(&self.role_model_name)
                            .show_ui(ui, |ui| {
                                for model in &self.available_models {
                                    ui.selectable_value(
                                        &mut self.role_model_name,
                                        model.clone(),
                                        model,
                                    );
                                }
                            });

                        ui.add_space(8.0);
                        ui.label("系统提示词:");
                        ui.text_edit_multiline(&mut self.role_prompt_input);

                        ui.add_space(8.0);
                        ui.label("Temperature:");
                        ui.add(
                            egui::Slider::new(&mut self.role_temperature, 0.0..=2.0).step_by(0.1),
                        );

                        ui.add_space(16.0);
                        if ui.small_button("创建角色").clicked()
                            && !self.role_name_input.trim().is_empty()
                        {
                            self.create_role();
                        }
                    });
                });
        }
    }
}
