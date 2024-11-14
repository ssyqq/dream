use serde::{Serialize, Deserialize};
use uuid::Uuid;
use serde_json::{json, Value as JsonValue};
use std::path::Path;
use crate::utils;

#[derive(Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
    pub image_path: Option<String>,
}

impl Message {
    pub async fn to_api_content(&self) -> std::io::Result<JsonValue> {
        match &self.image_path {
            Some(path) => {
                let base64_image = utils::get_image_base64(Path::new(path)).await?;
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

    pub fn new_user(content: String, image_path: Option<String>) -> Self {
        Self {
            role: "user".to_string(),
            content,
            image_path,
        }
    }

    pub fn new_assistant(content: String) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            image_path: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatHistory(pub Vec<Message>);

impl ChatHistory {
    pub fn add_message(&mut self, message: Message) {
        self.0.push(message);
    }

    pub fn last_message_is_assistant(&self) -> bool {
        self.0.last()
            .map(|msg| msg.role == "assistant")
            .unwrap_or(false)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Chat {
    pub id: String,
    pub name: String,
    pub messages: Vec<Message>,
    pub has_been_renamed: bool,
}

impl Chat {
    pub fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            messages: Vec::new(),
            has_been_renamed: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatList {
    pub chats: Vec<Chat>,
    pub current_chat_id: Option<String>,
}

impl Default for ChatList {
    fn default() -> Self {
        Self {
            chats: Vec::new(),
            current_chat_id: None,
        }
    }
} 