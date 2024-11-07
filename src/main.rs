use eframe::egui;
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

#[derive(Serialize, Deserialize, Clone)]
struct ChatHistory(Vec<(String, String)>);

#[derive(Serialize, Deserialize, Clone)]
struct Chat {
    id: String,
    name: String,
    messages: Vec<(String, String)>,
    has_been_renamed: bool,  // 添加重命名标记
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
    title_receiver: Option<mpsc::UnboundedReceiver<(String, String)>>,  // 新增字段用于存储标题接收器
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
            title_receiver: None,
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
        let mut config = toml::map::Map::new();
        
        // API 相关配置
        let mut api = toml::map::Map::new();
        api.insert("endpoint".to_string(), toml::Value::String(self.api_endpoint.clone()));
        api.insert("model".to_string(), toml::Value::String(self.model_name.clone()));
        config.insert("api".to_string(), toml::Value::Table(api));
        
        // Chat 相关配置
        let mut chat = toml::map::Map::new();
        chat.insert("system_prompt".to_string(), toml::Value::String(self.system_prompt.clone()));
        chat.insert("temperature".to_string(), toml::Value::Float(self.temperature as f64));
        config.insert("chat".to_string(), toml::Value::Table(chat));
        
        // API Key
        config.insert("api_key".to_string(), toml::Value::String(self.api_key.clone()));
        
        // 配置转换为 TOML 字符串
        let toml_string = toml::to_string_pretty(&toml::Value::Table(config))?;
        
        // 写入文件
        fs::write("dream.toml", toml_string)?;
        
        Ok(())
    }

    fn save_chat_list(&self) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self.chat_list)?;
        fs::write("chat_list.json", json)?;
        Ok(())
    }

    fn load_chat_list(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(content) = fs::read_to_string("chat_list.json") {
            self.chat_list = serde_json::from_str(&content)?;
            // 加载后反转列表顺序
            self.chat_list.chats.reverse();
        }
        Ok(())
    }

    fn new_chat(&mut self) {
        let id = Uuid::new_v4().to_string();
        let chat_count = self.chat_list.chats.len();
            
        let new_chat = Chat {
            id: id.clone(),
            name: format!("新对话 {}", chat_count + 1),
            messages: Vec::new(),
            has_been_renamed: false,  // 初始化为 false
        };
        // 将新对话插入到列表开头而不是末尾
        self.chat_list.chats.insert(0, new_chat);
        self.chat_list.current_chat_id = Some(id);
        self.chat_history.0.clear();
        let _ = self.save_chat_list();
    }

    async fn generate_title(&self, messages: &[(String, String)]) -> Result<String, Box<dyn std::error::Error + Send>> {
        // 构建用于生成标题的提示
        let content = messages.first()
            .map(|(user_msg, _)| user_msg.clone())
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

        let response_json: JsonValue = response.json()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
        let title = response_json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("新对话")
            .trim()
            .to_string();

        Ok(title)
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
                                if ui.button("新建对话").clicked() {
                                    self.new_chat();
                                }
                            });
                            
                            ui.separator();
                            
                            // 聊天列表区域 - 设置为��充剩余空间
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
                                            self.chat_history.0 = messages;
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
                            // 如果当前没有选中的对话，选中第一个
                            if let Some(first_chat) = self.chat_list.chats.first() {
                                self.chat_list.current_chat_id = Some(first_chat.id.clone());
                                self.chat_history.0 = first_chat.messages.clone();
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
                                });
                            
                            if config_changed {
                                let _ = self.save_config(frame);
                            }
                        });
                }

                // 聊天历史记录区域
                ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .stick_to_bottom(true)
                    .max_height(history_height)
                    .show(ui, |ui| {
                        for (i, (user_msg, ai_msg)) in self.chat_history.0.iter().enumerate() {
                            if i > 0 {
                                ui.add_space(4.0);
                                ui.separator();
                                ui.add_space(4.0);
                            }
                            
                            ui.label(egui::RichText::new("You: ").strong());
                            ui.label(user_msg);
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new("AI: ").strong());
                            ui.label(ai_msg);
                        }
                    });

                // 输入区域
                ui.horizontal(|ui| {
                    let available_width = ui.available_width();
                    let text_edit = TextEdit::multiline(&mut self.input_text)
                        .desired_rows(3)
                        .min_size(egui::vec2(available_width - 60.0, input_height));
                    
                    let text_edit_response = ui.add(text_edit);
                    
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
                        let button_height = input_height;
                        if ui.add_sized(
                            [50.0, button_height], 
                            egui::Button::new(if self.is_sending { "发送中..." } else { "发送" })
                        ).clicked() || (ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
                            && text_edit_response.has_focus())
                        {
                            if !self.input_text.is_empty() && !self.is_sending && !self.api_key.is_empty() {
                                // 如果没有选中的聊天，创建一个新的
                                if self.chat_list.current_chat_id.is_none() {
                                    self.new_chat();
                                }

                                let user_input = std::mem::take(&mut self.input_text);
                                self.is_sending = true;
                                
                                let api_key = self.api_key.clone();
                                let api_endpoint = self.api_endpoint.clone();
                                let model_name = self.model_name.clone();
                                let system_prompt = self.system_prompt.clone();
                                let temperature = self.temperature;
                                let client = self.client.clone();
                                
                                // 克隆历史消息
                                let chat_history = self.chat_history.0.clone();
                                self.chat_history.0.push((user_input.clone(), String::new()));
                                
                                let (tx, rx) = mpsc::unbounded_channel();
                                self.receiver = Some(rx);
                                
                                let ctx = ctx.clone();
                                
                                self.runtime.spawn(async move {
                                    // 构建消息历史
                                    let mut messages = vec![
                                        json!({
                                            "role": "system",
                                            "content": system_prompt
                                        })
                                    ];

                                    // 使用克隆的历史消息
                                    for (user_msg, ai_msg) in chat_history.iter() {
                                        messages.push(json!({
                                            "role": "user",
                                            "content": user_msg
                                        }));
                                        messages.push(json!({
                                            "role": "assistant",
                                            "content": ai_msg
                                        }));
                                    }

                                    // 添加当前用户消息
                                    messages.push(json!({
                                        "role": "user",
                                        "content": user_input
                                    }));

                                    let response = client
                                        .post(&api_endpoint)
                                        .header("Authorization", format!("Bearer {}", api_key))
                                        .header("Content-Type", "application/json")
                                        .json(&json!({
                                            "model": model_name,
                                            "messages": messages,
                                            "temperature": temperature,
                                            "stream": true
                                        }))
                                        .send()
                                        .await;

                                    match response {
                                        Ok(res) => {
                                            let mut stream = res.bytes_stream();
                                            let mut current_message = String::new();
                                            
                                            while let Some(chunk_result) = stream.next().await {
                                                if let Ok(chunk) = chunk_result {
                                                    if let Ok(text) = String::from_utf8(chunk.to_vec()) {
                                                        // 处理 SSE 数据
                                                        for line in text.lines() {
                                                            if line.starts_with("data: ") {
                                                                let data = &line[6..];
                                                                if data == "[DONE]" {
                                                                    // 发送一个特殊标来表示流式响应结束
                                                                    let _ = tx.send("__STREAM_DONE__".to_string());
                                                                    continue;
                                                                }
                                                                if let Ok(json) = serde_json::from_str::<JsonValue>(data) {
                                                                    if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                                                                        current_message.push_str(content);
                                                                        let _ = tx.send(current_message.clone());
                                                                        ctx.request_repaint();
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            let _ = tx.send(format!("API 请求失败: {}", e));
                                            let _ = tx.send("__STREAM_DONE__".to_string()); // 错误时也发送结束标记
                                        }
                                    }
                                });
                            }
                        }
                    });
                });
            });

            // 处理主消息接收器
            let mut should_save = false;
            if let Some(receiver) = &mut self.receiver {
                while let Ok(response) = receiver.try_recv() {
                    match response.as_str() {
                        "__STREAM_DONE__" => {
                            self.is_sending = false;
                            // 保存当前对话的消息
                            if let Some(current_id) = &self.chat_list.current_chat_id {
                                if let Some(chat) = self.chat_list.chats
                                    .iter_mut()
                                    .find(|c| &c.id == current_id)
                                {
                                    chat.messages = self.chat_history.0.clone();
                                    
                                    // 如果是第一条消息且未重命名，启动重命名任务
                                    if chat.messages.len() == 1 && !chat.has_been_renamed {
                                        let api_key = self.api_key.clone();
                                        let api_endpoint = self.api_endpoint.clone();
                                        let model_name = self.model_name.clone();
                                        let messages = chat.messages.clone();
                                        let client = self.client.clone();
                                        let chat_id = current_id.clone();
                                        
                                        // 创建标题更新通道
                                        let (title_tx, title_rx) = mpsc::unbounded_channel();
                                        self.title_receiver = Some(title_rx);
                                        let ctx = ctx.clone();
                                        
                                        self.runtime.spawn(async move {
                                            // 构建用于生成标题的提示
                                            let content = messages.first()
                                                .map(|(user_msg, _)| user_msg.clone())
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

                                            let response = client
                                                .post(&api_endpoint)
                                                .header("Authorization", format!("Bearer {}", api_key))
                                                .header("Content-Type", "application/json")
                                                .json(&json!({
                                                    "model": model_name,
                                                    "messages": messages,
                                                    "temperature": 0.7,
                                                    "max_tokens": 60
                                                }))
                                                .send()
                                                .await;

                                            if let Ok(response) = response {
                                                if let Ok(json) = response.json::<JsonValue>().await {
                                                    if let Some(title) = json["choices"][0]["message"]["content"]
                                                        .as_str()
                                                        .map(|s| s.trim().to_string())
                                                    {
                                                        let _ = title_tx.send((chat_id, title));
                                                        ctx.request_repaint();
                                                    }
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                            should_save = true;
                        }
                        _ => {
                            if let Some(last_msg) = self.chat_history.0.last_mut() {
                                last_msg.1 = response;
                            }
                        }
                    }
                }
            }

            // 处理标题更新
            let mut title_updated = false;
            if let Some(title_rx) = &mut self.title_receiver {
                while let Ok((chat_id, new_title)) = title_rx.try_recv() {
                    if let Some(chat) = self.chat_list.chats
                        .iter_mut()
                        .find(|c| c.id == chat_id)
                    {
                        chat.name = new_title;
                        chat.has_been_renamed = true;
                        title_updated = true;
                    }
                }
            }

            // 在所有处理完成后进行保存
            if should_save || title_updated {
                let _ = self.save_chat_list();
            }

            // 保存配置
            let _ = self.save_config(frame);
        });
    }
}

fn main() -> Result<(), eframe::Error> {
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

            // 将中文字体设置为优先字体
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
