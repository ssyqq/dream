use std::path::{Path, PathBuf};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use uuid::Uuid;
use std::io;
use env_logger::Builder;
use chrono::Local;
use std::io::Write;
use log::LevelFilter;

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

pub fn ensure_cache_dir() -> io::Result<PathBuf> {
    let cache_dir = PathBuf::from(".cache/images");
    std::fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir)
}

pub fn copy_to_cache(source_path: &Path) -> Result<PathBuf, ImageError> {
    let cache_dir = ensure_cache_dir()?;
    let file_name = format!("{}.jpg", Uuid::new_v4());
    let cache_path = cache_dir.join(&file_name);
    
    let img = image::open(source_path)
        .map_err(ImageError::ImageError)?;
    
    img.save(&cache_path)
        .map_err(ImageError::ImageError)?;
    
    Ok(cache_path)
}

pub fn get_image_base64(path: &Path) -> io::Result<String> {
    let image_data = std::fs::read(path)?;
    Ok(BASE64.encode(&image_data))
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