//! 数据持久化模块
//!
//! 负责将爬取结果保存到不同格式的文件。
//! 使用 trait 抽象存储后端，支持 JSON、CSV 和 SQLite 三种格式。

use crate::models::Page;
use async_trait::async_trait;
use rusqlite::params;
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
/// 将页面数据保存为标准 JSON 数组格式 `[{...}, {...}, ...]`。
pub struct JsonStorage {
    #[allow(dead_code)]
    path: std::path::PathBuf,
    writer: Arc<Mutex<BufWriter<File>>>,
    /// 是否尚未写入任何页面（用于逗号分隔）
    first_page: std::sync::Mutex<bool>,
}

impl JsonStorage {
    /// 创建新的 JSON 存储
    ///
    /// 创建文件并写入数组开头 `[`。
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();

        // 创建文件（如果存在则清空）
        let file = File::create(&path)?;
        let mut writer = BufWriter::new(file);
        // 写入 JSON 数组起始符
        writer.write_all(b"[\n")?;
        writer.flush()?;

        Ok(Self {
            path,
            writer: Arc::new(Mutex::new(writer)),
            first_page: std::sync::Mutex::new(true),
        })
    }
}

#[async_trait]
impl Storage for JsonStorage {
    async fn save_page(&self, page: &Page) -> Result<(), StorageError> {
        // 序列化页面为 JSON（已压缩为单行）
        let json = serde_json::to_string(page)?;

        let mut writer = self.writer.lock().await;
        let mut is_first = self.first_page.lock().unwrap();

        if !*is_first {
            // 非首条，追加逗号分隔
            writer.write_all(b",\n")?;
        }
        *is_first = false;

        // 写入当前页（缩进 2 空格，保持可读性）
        writer.write_all(b"  ")?;
        writer.write_all(json.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;

        Ok(())
    }

    async fn close(&self) -> Result<(), StorageError> {
        // 写入数组结束符并刷新
        let mut writer = self.writer.lock().await;
        writer.write_all(b"]\n")?;
        writer.flush()?;

        // 显式 drop 以确保文件被关闭
        drop(writer);

        Ok(())
    }
}

/// CSV 存储实现
///
/// 将页面数据保存为 CSV 格式，每行一个页面。
/// 链接信息以 JSON 数组形式存储在 links 列中。
pub struct CsvStorage {
    #[allow(dead_code)]
    path: std::path::PathBuf,
    writer: std::sync::Mutex<csv::Writer<BufWriter<File>>>,
}

impl CsvStorage {
    /// 创建新的 CSV 存储
    ///
    /// 创建 CSV 文件并写入表头。
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();

        // 创建文件
        let file = File::create(&path)?;
        let buf_writer = BufWriter::new(file);
        let mut writer = csv::Writer::from_writer(buf_writer);

        // 写入表头
        writer.write_record([
            "url",
            "title",
            "content",
            "status_code",
            "depth",
            "links_json",
            "fetch_duration_ms",
            "crawled_at",
        ])?;
        writer.flush()?;

        Ok(Self {
            path,
            writer: std::sync::Mutex::new(writer),
        })
    }
}

#[async_trait]
impl Storage for CsvStorage {
    async fn save_page(&self, page: &Page) -> Result<(), StorageError> {
        // 将链接序列化为 JSON
        let links_json = serde_json::to_string(&page.links)?;

        // 使用 std::sync::Mutex 保护 writer，lock 后立即完成同步写入
        let mut writer = self.writer.lock().unwrap();
        writer.write_record([
            &page.url,
            page.title.as_deref().unwrap_or(""),
            page.content.as_deref().unwrap_or(""),
            &page.status_code.to_string(),
            &page.depth.to_string(),
            &links_json,
            &page.fetch_duration_ms.to_string(),
            &page
                .crawled_at
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string(),
        ])?;

        Ok(())
    }

    async fn close(&self) -> Result<(), StorageError> {
        let mut writer = self.writer.lock().unwrap();
        writer.flush()?;
        Ok(())
    }
}

/// SQLite 存储实现
///
/// 将页面数据保存到 SQLite 数据库。
/// 使用两张表：pages（页面信息）和 links（链接信息）。
pub struct SqliteStorage {
    #[allow(dead_code)]
    path: std::path::PathBuf,
    conn: std::sync::Mutex<rusqlite::Connection>,
}

impl SqliteStorage {
    /// 创建新的 SQLite 存储
    ///
    /// 创建数据库文件并初始化表结构。
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        let conn = rusqlite::Connection::open(&path)?;

        // 初始化 pages 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS pages (
                url TEXT PRIMARY KEY,
                title TEXT,
                content TEXT,
                status_code INTEGER,
                depth INTEGER,
                fetch_duration_ms INTEGER,
                crawled_at INTEGER
            )",
            [],
        )?;

        // 初始化 links 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                page_url TEXT NOT NULL,
                link_text TEXT,
                link_url TEXT NOT NULL,
                is_internal INTEGER,
                FOREIGN KEY (page_url) REFERENCES pages(url)
            )",
            [],
        )?;

        Ok(Self {
            path,
            conn: std::sync::Mutex::new(conn),
        })
    }
}

#[async_trait]
impl Storage for SqliteStorage {
    async fn save_page(&self, page: &Page) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();

        // 插入页面信息
        conn.execute(
            "INSERT OR REPLACE INTO pages
             (url, title, content, status_code, depth, fetch_duration_ms, crawled_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                &page.url,
                page.title.as_deref(),
                page.content.as_deref(),
                page.status_code,
                page.depth,
                page.fetch_duration_ms,
                page.crawled_at
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
            ],
        )?;

        // 插入该页面的所有链接
        for link in &page.links {
            conn.execute(
                "INSERT INTO links (page_url, link_text, link_url, is_internal)
                 VALUES (?1, ?2, ?3, ?4)",
                params![&page.url, &link.text, &link.url, link.is_internal as i32,],
            )?;
        }

        Ok(())
    }

    async fn close(&self) -> Result<(), StorageError> {
        // SQLite 在 drop Connection 时会自动关闭，这里额外执行 VACUUM 优化
        let conn = self.conn.lock().unwrap();
        conn.execute("VACUUM", [])?;
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
        crate::config::OutputFormat::Json => Ok(Box::new(JsonStorage::new(path)?)),
        crate::config::OutputFormat::Csv => Ok(Box::new(CsvStorage::new(path)?)),
        crate::config::OutputFormat::Sqlite => Ok(Box::new(SqliteStorage::new(path)?)),
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
        let page = Page::new("https://example.com".to_string(), 200, 0, 100);

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

        let mut page = Page::new("https://example.com/page1".to_string(), 200, 0, 150);
        page.title = Some("Example Page".to_string());
        page.content = Some("This is the content".to_string());

        // 保存多个页面
        assert!(storage.save_page(&page).await.is_ok());

        let mut page2 = Page::new("https://example.com/page2".to_string(), 200, 1, 120);
        page2.title = Some("Another Page".to_string());

        assert!(storage.save_page(&page2).await.is_ok());

        // 关闭存储（刷新缓冲区）
        assert!(storage.close().await.is_ok());

        // 验证文件存在
        assert!(file_path.exists());

        // 读取文件内容
        let content = std::fs::read_to_string(&file_path).unwrap();

        // 应该能作为 JSON 数组整体反序列化
        let pages: Vec<Page> = serde_json::from_str(&content).unwrap();
        assert_eq!(pages.len(), 2);

        assert_eq!(pages[0].url, "https://example.com/page1");
        assert_eq!(pages[0].title, Some("Example Page".to_string()));

        assert_eq!(pages[1].url, "https://example.com/page2");
        assert_eq!(pages[1].title, Some("Another Page".to_string()));

        // 同时验证首行是 [，末行是 ]
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[0].trim(), "[");
        assert_eq!(lines.last().map(|s| s.trim()), Some("]"));
    }
}
