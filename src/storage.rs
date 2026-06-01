//! 数据持久化模块
//!
//! 负责将爬取结果保存到不同格式的文件。
//! 使用 trait 抽象存储后端，支持 JSON、CSV 和 SQLite 三种格式。

use crate::models::Page;
use async_trait::async_trait;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

/// 存储后端 trait
///
/// 定义了存储操作的标准接口，允许不同的持久化实现。
#[async_trait]
pub trait Storage: Send + Sync {
    /// 保存单个页面
    ///
    /// # 参数
    /// - `page`: 要保存的页面数据
    async fn save_page(&self, page: &Page) -> Result<(), StorageError>;

    /// 关闭存储，刷新所有缓存数据
    async fn close(&self) -> Result<(), StorageError>;
}

/// 存储错误类型
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("IO 错误: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON 序列化错误: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("CSV 写入错误: {0}")]
    CsvError(#[from] csv::Error),

    #[error("SQLite 错误: {0}")]
    SqliteError(#[from] rusqlite::Error),

    #[error("无效的文件路径: {0}")]
    InvalidPath(String),
}

/// JSON 存储实现
///
/// 将页面数据保存为 JSON Lines 格式（每行一个 JSON 对象）。
pub struct JsonStorage {
    #[allow(dead_code)]
    path: std::path::PathBuf,
    writer: Arc<Mutex<BufWriter<File>>>,
}

impl JsonStorage {
    /// 创建新的 JSON 存储
    ///
    /// 创建一个新的文件，如果文件已存在则覆盖。
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();

        // 创建文件（如果存在则清空）
        let file = File::create(&path)?;
        let writer = BufWriter::new(file);

        Ok(Self {
            path,
            writer: Arc::new(Mutex::new(writer)),
        })
    }
}

#[async_trait]
impl Storage for JsonStorage {
    async fn save_page(&self, page: &Page) -> Result<(), StorageError> {
        // 序列化页面为 JSON
        let json = serde_json::to_string(page)?;

        // 获取写入器的锁并写入
        // 注意：这会在 async 上下文中短时间阻塞，但 BufWriter 的写入很快，通常不是问题
        let mut writer = self.writer.lock().await;
        let json_line = format!("{}\n", json);
        writer.write_all(json_line.as_bytes())?;

        Ok(())
    }

    async fn close(&self) -> Result<(), StorageError> {
        // 刷新缓冲区
        let mut writer = self.writer.lock().await;
        writer.flush()?;

        // 显式 drop 以确保文件被关闭
        drop(writer);

        Ok(())
    }
}

/// CSV 存储实现
///
/// 将页面数据保存为 CSV 格式。
pub struct CsvStorage {
    #[allow(dead_code)]
    path: std::path::PathBuf,
    // TODO: 添加 CSV writer
}

impl CsvStorage {
    /// 创建新的 CSV 存储
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        Ok(Self { path })
    }
}

#[async_trait]
impl Storage for CsvStorage {
    async fn save_page(&self, _page: &Page) -> Result<(), StorageError> {
        // TODO: 实现 CSV 写入
        Ok(())
    }

    async fn close(&self) -> Result<(), StorageError> {
        // TODO: 刷新缓冲区
        Ok(())
    }
}

/// SQLite 存储实现
///
/// 将页面数据保存到 SQLite 数据库。
pub struct SqliteStorage {
    #[allow(dead_code)]
    path: std::path::PathBuf,
    // TODO: 添加数据库连接
}

impl SqliteStorage {
    /// 创建新的 SQLite 存储
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        // TODO: 初始化数据库表
        Ok(Self { path })
    }
}

#[async_trait]
impl Storage for SqliteStorage {
    async fn save_page(&self, _page: &Page) -> Result<(), StorageError> {
        // TODO: 实现 SQLite 插入
        Ok(())
    }

    async fn close(&self) -> Result<(), StorageError> {
        // TODO: 关闭数据库连接
        Ok(())
    }
}

/// 存储工厂函数
///
/// 根据输出格式创建对应的存储实现。
pub fn create_storage(
    format: crate::config::OutputFormat,
    path: &Path,
) -> Result<Box<dyn Storage>, StorageError> {
    match format {
        crate::config::OutputFormat::Json => {
            Ok(Box::new(JsonStorage::new(path)?))
        }
        crate::config::OutputFormat::Csv => {
            Ok(Box::new(CsvStorage::new(path)?))
        }
        crate::config::OutputFormat::Sqlite => {
            Ok(Box::new(SqliteStorage::new(path)?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Page;
    use std::path::PathBuf;

    #[test]
    fn test_create_json_storage() {
        let storage = JsonStorage::new(PathBuf::from("test.json"));
        assert!(storage.is_ok());
    }

    #[test]
    fn test_create_csv_storage() {
        let storage = CsvStorage::new(PathBuf::from("test.csv"));
        assert!(storage.is_ok());
    }

    #[test]
    fn test_create_sqlite_storage() {
        let storage = SqliteStorage::new(PathBuf::from("test.db"));
        assert!(storage.is_ok());
    }

    #[tokio::test]
    async fn test_storage_trait_object() {
        let page = Page::new(
            "https://example.com".to_string(),
            200,
            0,
            100,
        );

        let storage: Box<dyn Storage> =
            Box::new(JsonStorage::new(PathBuf::from("test.json")).unwrap());

        // 保存页面
        assert!(storage.save_page(&page).await.is_ok());
        assert!(storage.close().await.is_ok());
    }

    #[tokio::test]
    async fn test_json_storage_writes_file() {
        use tempfile::tempdir;

        // 创建临时目录
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("output.json");

        // 创建存储并保存页面
        let storage = JsonStorage::new(&file_path).unwrap();

        let mut page = Page::new(
            "https://example.com/page1".to_string(),
            200,
            0,
            150,
        );
        page.title = Some("Example Page".to_string());
        page.content = Some("This is the content".to_string());

        // 保存多个页面
        assert!(storage.save_page(&page).await.is_ok());

        let mut page2 = Page::new(
            "https://example.com/page2".to_string(),
            200,
            1,
            120,
        );
        page2.title = Some("Another Page".to_string());

        assert!(storage.save_page(&page2).await.is_ok());

        // 关闭存储（刷新缓冲区）
        assert!(storage.close().await.is_ok());

        // 验证文件存在
        assert!(file_path.exists());

        // 读取文件内容
        let content = std::fs::read_to_string(&file_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        // 应该有 2 行 JSON
        assert_eq!(lines.len(), 2);

        // 验证可以反序列化
        let first_page: Page = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first_page.url, "https://example.com/page1");
        assert_eq!(first_page.title, Some("Example Page".to_string()));

        let second_page: Page = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second_page.url, "https://example.com/page2");
        assert_eq!(second_page.title, Some("Another Page".to_string()));
    }
}
