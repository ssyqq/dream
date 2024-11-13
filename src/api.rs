use reqwest::Client;
use serde_json::Value as JsonValue;
use tokio::sync::mpsc;
use futures_util::StreamExt;
use log::{debug, error};

#[derive(Debug)]
pub enum ApiError {
    TooManyRequests(reqwest::Response),
    Other(reqwest::Error),
    HttpError(reqwest::Response),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::TooManyRequests(_) => write!(f, "请求频率限制"),
            ApiError::Other(e) => write!(f, "请求错误: {}", e),
            ApiError::HttpError(res) => write!(f, "HTTP错误: {}", res.status()),
        }
    }
}

impl std::error::Error for ApiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ApiError::TooManyRequests(_) => None,
            ApiError::Other(e) => Some(e),
            ApiError::HttpError(_) => None,
        }
    }
}

pub async fn send_request(
    client: &Client,
    api_endpoint: &str,
    api_key: &str,
    payload: &JsonValue,
    retry_enabled: bool,
    max_retries: i32,
    tx: &mpsc::UnboundedSender<String>,
) -> Result<(), ApiError> {
    let mut retry_count = 0;
    let mut incomplete_data = String::new();
    
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
                    debug!("遇到 429 错误，即将进行第 {} 次重试", retry_count);
                    let _ = tx.send(format!("遇到频率限制，正在进行第 {} 次重试...", retry_count));
                    continue;
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
                        for line in text.lines() {
                            
                            incomplete_data.push_str(line);
                            // debug!("处理数据行: {}", incomplete_data);

                            if incomplete_data.contains("data: ") {
                                let index = incomplete_data.find("data: ").unwrap();
                                let data = &incomplete_data[index + 6..];
                                if data == "[DONE]" {
                                    debug!("收到结束标记: [DONE]");
                                    let _ = tx.send("__STREAM_DONE__".to_string());
                                    return Ok(());
                                }

                                match serde_json::from_str::<JsonValue>(data) {
                                    Ok(json) => {
                                        incomplete_data.clear();
                                        
                                        if let Some(error) = json.get("error") {
                                            if retry_enabled && retry_count < max_retries {
                                                retry_count += 1;
                                                debug!("遇到API错误，立即进行第 {} 次重试", retry_count);
                                                let _ = tx.send(format!("遇到API错误，正在进行第 {} 次重试...", retry_count));
                                                continue;
                                            } else {
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
                                                    format!("API错误 (重试{}次后): {}", 
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
                                    Err(_) => {
                                        debug!("JSON解析失败（数据不完整）");
                                        continue;
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
                        continue;
                    }
                    return Err(ApiError::Other(e.into()));
                }
            }
        }
        
        return Ok(());
    }
} 