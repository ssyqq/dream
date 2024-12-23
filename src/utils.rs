use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::Local;
use env_logger::Builder;
use image::GenericImageView;
use log::{debug, error, LevelFilter};
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::fs;
use tokio::task;
use uuid::Uuid;

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
        let img = image::load_from_memory(&image_data).map_err(ImageError::ImageError)?;
        debug!("图片加载耗时: {:?}", load_start.elapsed());

        // 计时：转换格式和压缩
        let convert_start = Instant::now();
        // 转换为RGB8
        let rgb_img = img.into_rgb8();
        debug!("转换RGB格式耗时: {:?}", convert_start.elapsed());

        // 计时：保存图片（使用较低的JPEG质量）
        let save_start = Instant::now();
        let mut output = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 85);
        encoder
            .encode(
                rgb_img.as_raw(),
                rgb_img.width(),
                rgb_img.height(),
                image::ColorType::Rgb8.into(),
            )
            .map_err(ImageError::ImageError)?;
        debug!("编码图片耗时: {:?}", save_start.elapsed());

        Ok::<(Vec<u8>, PathBuf), ImageError>((output, cache_path_clone))
    })
    .await
    .unwrap()?;

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
    })
    .await
    .unwrap();

    debug!("总耗时: {:?}", start.elapsed());
    Ok(encoded)
}

pub fn setup_logger() {
    Builder::from_default_env()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                record.level(),
                record.args()
            )
        })
        .filter_level(LevelFilter::Debug)
        .init();
}

pub async fn remove_cached_image(path: &str) -> std::io::Result<()> {
    debug!("尝试删除缓存图片: {}", path);

    // 获取缓存目录的绝对路径
    let cache_dir = ensure_cache_dir().await?;
    let cache_dir = cache_dir.canonicalize()?;
    debug!("缓存目录: {:?}", cache_dir);

    // 获取图片的绝对路径
    let path = std::path::PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    };
    let path = path.canonicalize()?;
    debug!("图片路径: {:?}", path);

    // 将两个路径转换为字符串进行比较
    let cache_str = cache_dir.to_string_lossy().to_lowercase();
    let path_str = path.to_string_lossy().to_lowercase();

    if path_str.contains(&cache_str) {
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
        debug!(
            "图片不在缓存目录中，跳过删除。\n缓存目录: {}\n图片路径: {}",
            cache_str, path_str
        );
        Ok(())
    }
}
