use serde::{Serialize, Deserialize};
use std::error::Error as StdError;
use tokio::fs;

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub api_key: String,
    pub api: ApiConfig,
    pub chat: ChatConfig,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiConfig {
    pub endpoint: String,
    pub model: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ChatConfig {
    pub system_prompt: String,
    pub temperature: f64,
    pub retry_enabled: bool,
    pub max_retries: i64,
    pub dark_mode: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api: ApiConfig {
                endpoint: "https://api.openai.com/v1/chat/completions".to_string(),
                model: "gpt-3.5-turbo".to_string(),
            },
            chat: ChatConfig {
                system_prompt: "你是一个有帮助的助手。".to_string(),
                temperature: 0.7,
                retry_enabled: true,
                max_retries: 10,
                dark_mode: true,
            },
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    IoError(std::io::Error),
    TomlError(toml::ser::Error),
    ParseError(toml::de::Error),
}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        ConfigError::IoError(err)
    }
}

impl From<toml::ser::Error> for ConfigError {
    fn from(err: toml::ser::Error) -> Self {
        ConfigError::TomlError(err)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(err: toml::de::Error) -> Self {
        ConfigError::ParseError(err)
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::IoError(e) => write!(f, "IO错误: {}", e),
            ConfigError::TomlError(e) => write!(f, "TOML序列化错误: {}", e),
            ConfigError::ParseError(e) => write!(f, "TOML解析错误: {}", e),
        }
    }
}

impl StdError for ConfigError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            ConfigError::IoError(e) => Some(e),
            ConfigError::TomlError(e) => Some(e),
            ConfigError::ParseError(e) => Some(e),
        }
    }
}

pub async fn load_config() -> Config {
    match fs::read_to_string("dream.toml").await {
        Ok(content) => toml::from_str(&content).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

pub async fn save_config(config: &Config) -> Result<(), ConfigError> {
    let toml_string = toml::to_string_pretty(config)?;
    fs::write("dream.toml", toml_string).await?;
    Ok(())
} 