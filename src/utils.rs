use std::path::{Path, PathBuf};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use uuid::Uuid;
use std::io;
use env_logger::Builder;
use chrono::Local;
use std::io::Write;
use log::{LevelFilter, debug, error};
use std::time::Instant;
use image::GenericImageView;
use tokio::fs;
use tokio::task;

#[derive(Debug)]
pub enum ImageError {
    IoError(io::Error),
    ImageError(image::ImageError),
}

impl From<io::Error> for ImageError {
    fn from(err: io::Error) -> Self {
        ImageError::IoError(err)
    }
}

impl From<image::ImageError> for ImageError {
    fn from(err: image::ImageError) -> Self {
        ImageError::ImageError(err)
    }
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::IoError(e) => write!(f, "IO错误: {}", e),
            ImageError::ImageError(e) => write!(f, "图片处理错误: {}", e),
        }
    }
}

impl std::error::Error for ImageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ImageError::IoError(e) => Some(e),
            ImageError::ImageError(e) => Some(e),
        }
    }
}

pub async fn ensure_cache_dir() -> io::Result<PathBuf> {
    let cache_dir = PathBuf::from(".cache/images");
    fs::create_dir_all(&cache_dir).await?;
    Ok(cache_dir)
}

pub async fn copy_to_cache(source_path: &Path) -> Result<PathBuf, ImageError> {
    let start = Instant::now();
    
    let cache_dir = ensure_cache_dir().await?;
    let file_name = format!("{}.jpg", Uuid::new_v4());
    let cache_path = cache_dir.join(&file_name);
    
    // 异步读取源文件
    let image_data = fs::read(source_path).await?;
    
    // 直接使用 spawn_blocking 处理 CPU 密集型任务
    let cache_path_clone = cache_path.clone();
    let processed_image = task::spawn_blocking(move || {
        // 计时：图片加载
        let load_start = Instant::now();
        let img = image::load_from_memory(&image_data)
            .map_err(ImageError::ImageError)?;
        debug!("图片加载耗时: {:?}", load_start.elapsed());
        
        // 计时：转换格式和压缩
        let convert_start = Instant::now();
        
        // 获取原始尺寸
        let (width, height) = img.dimensions();
        
        // 先缩放，再转换格式，减少处理的数据量
        let img = if width > 800 || height > 800 {
            let scale = 800.0 / width.max(height) as f32;
            let new_width = (width as f32 * scale) as u32;
            let new_height = (height as f32 * scale) as u32;
            img.resize(new_width, new_height, image::imageops::FilterType::Nearest)
        } else {
            img
        };
        
        // 转换为RGB8
        let rgb_img = img.into_rgb8();
        debug!("转换RGB格式耗时: {:?}", convert_start.elapsed());
        
        // 计时：保存图片（使用较低的JPEG质量）
        let save_start = Instant::now();
        let mut output = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 85);
        encoder.encode(
            rgb_img.as_raw(),
            rgb_img.width(),
            rgb_img.height(),
            image::ColorType::Rgb8.into(),
        ).map_err(ImageError::ImageError)?;
        debug!("编码图片耗时: {:?}", save_start.elapsed());
        
        Ok::<(Vec<u8>, PathBuf), ImageError>((output, cache_path_clone))
    }).await.unwrap()?;
    
    // 异步写入处理后的图片
    fs::write(processed_image.1, processed_image.0).await?;
    
    debug!("总耗时: {:?}", start.elapsed());
    Ok(cache_path)
}

pub async fn get_image_base64(path: &Path) -> io::Result<String> {
    let start = Instant::now();
    
    debug!("开始读取图片文件: {:?}", path);
    let read_start = Instant::now();
    let image_data = fs::read(path).await?;
    debug!("读取图片耗时: {:?}", read_start.elapsed());
    
    // Base64 编码在单独的线程中进行
    let encoded = task::spawn_blocking(move || {
        debug!("图片文件大小: {} bytes", image_data.len());
        let encode_start = Instant::now();
        let encoded = BASE64.encode(&image_data);
        debug!("Base64编码耗时: {:?}", encode_start.elapsed());
        encoded
    }).await.unwrap();
    
    debug!("总耗时: {:?}", start.elapsed());
    Ok(encoded)
}

pub fn setup_logger() {
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
        .filter_level(LevelFilter::Debug)
        .init();
}

pub async fn remove_cached_image(path: &str) -> std::io::Result<()> {
    debug!("尝试删除缓存图片: {}", path);
    if let Ok(path) = std::path::PathBuf::from(path).canonicalize() {
        let cache_dir = ensure_cache_dir().await?;
        debug!("缓存目录: {:?}", cache_dir);
        
        if path.starts_with(&cache_dir) {
            debug!("确认图片在缓存目录中，开始删除: {:?}", path);
            match tokio::fs::remove_file(&path).await {
                Ok(_) => {
                    debug!("成功删除缓存图片: {:?}", path);
                    Ok(())
                }
                Err(e) => {
                    error!("删除缓存图片失败: {:?} - {}", path, e);
                    Err(e)
                }
            }
        } else {
            debug!("图片不在缓存目录中，跳过删除: {:?}", path);
            Ok(())
        }
    } else {
        debug!("无法解析图片路径: {}", path);
        Ok(())
    }
} 