# Rust 异步爬虫系统 — 设计与实现文档

> 适用：Rust 程序设计期末作业（单人项目）
> 目标代码量：2000~3000 行有效 Rust 代码

---

## 1. 项目概述

### 1.1 项目名称
**`rust-crawler`** — 一个基于异步运行时的高性能网页爬虫

### 1.2 核心功能

| 功能模块 | 说明 |
|---------|------|
| **种子抓取** | 从初始 URL 开始递归抓取网页 |
| **链接提取** | 解析 HTML，提取所有合法链接继续爬取 |
| **内容提取** | 提取页面标题、正文、元数据 |
| **并发控制** | 限制最大并发请求数，防止过载 |
| **URL 去重** | 避免重复抓取相同页面 |
| **深度限制** | 控制爬取层级深度 |
| **域名过滤** | 白名单/黑名单机制 |
| **数据导出** | 支持 JSON / CSV / SQLite 三种格式 |
| **进度展示** | 终端实时显示爬取进度和统计 |
| **优雅退出** | 支持 Ctrl+C 信号，保存已抓取数据 |

### 1.3 技术选型

| 用途 | 依赖库 | 版本 |
|------|--------|------|
| 异步运行时 | `tokio` | ^1.0 |
| HTTP 客户端 | `reqwest` | ^0.12 |
| HTML 解析 | `scraper` (或 `select`) | ^0.19 |
| 命令行参数 | `clap` | ^4.0 |
| 序列化 | `serde` + `serde_json` | ^1.0 |
| CSV 导出 | `csv` | ^1.3 |
| 数据库 | `rusqlite` | ^0.32 |
| 终端显示 | `indicatif` (进度条) | ^0.17 |
| 日志 | `tracing` + `tracing-subscriber` | ^0.1 |
| 配置解析 | `toml` | ^0.8 |

---

## 2. 架构设计

### 2.1 模块层次图

```
┌─────────────────────────────────────────┐
│              CLI 入口层                  │  main.rs
│         (clap 参数解析 + 启动)            │
├─────────────────────────────────────────┤
│              调度引擎层                  │  crawler.rs
│    (协调 fetcher/parser/storage 工作)    │
├─────────────────────────────────────────┤
│  ┌─────────┐ ┌─────────┐ ┌──────────┐  │
│  │ Fetcher │ │ Parser  │ │ Storage  │  │  fetcher.rs / parser.rs / storage.rs
│  │ HTTP获取 │ │ HTML解析 │ │ 数据持久化 │  │
│  └─────────┘ └─────────┘ └──────────┘  │
├─────────────────────────────────────────┤
│              基础设施层                  │
│  ┌─────────┐ ┌─────────┐ ┌──────────┐  │
│  │  Config │ │ Deduper │ │  Filter  │  │  config.rs / deduper.rs / filter.rs
│  │  配置   │ │ URL去重 │ │ 过滤策略  │  │
│  └─────────┘ └─────────┘ └──────────┘  │
├─────────────────────────────────────────┤
│              数据模型层                  │  models.rs
│         (Page / Link / Stats)           │
└─────────────────────────────────────────┘
```

### 2.2 并发模型（核心设计）

采用 **生产者-消费者模式** + **信号量限流**：

```
                    ┌─────────────────┐
                    │   URL 待抓取队列  │
                    │  (tokio::mpsc)  │
                    └────────┬────────┘
                             │
         ┌───────────────────┼───────────────────┐
         │                   │                   │
    ┌────▼────┐        ┌─────▼────┐        ┌────▼────┐
    │ Fetcher │        │ Fetcher  │        │ Fetcher │  ← Semaphore 控制并发数
    │ 任务1   │        │  任务2   │        │  任务N   │
    └────┬────┘        └─────┬────┘        └────┬────┘
         │                   │                   │
         └───────────────────┼───────────────────┘
                             │
                    ┌────────▼────────┐
                    │  结果通道 (mpsc)  │
                    │  Page 结构化数据  │
                    └────────┬────────┘
                             │
                    ┌────────▼────────┐
                    │  Storage 消费者   │  ← 单任务，顺序写入
                    │  (JSON/CSV/DB)   │
                    └─────────────────┘
```

**关键并发原语**：
- `tokio::sync::Semaphore` — 限制最大并发请求数
- `tokio::sync::mpsc` — URL 任务队列和结果队列
- `tokio::sync::RwLock<HashSet>` — 线程安全的 URL 去重表
- `tokio::task::JoinSet` — 管理所有抓取任务的生命周期

---

## 3. 数据结构设计

### 3.1 核心模型 (`src/models.rs`)

```rust
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};
use url::Url;

/// 爬取到的页面数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    /// 页面 URL（规范化后的绝对 URL）
    pub url: String,
    /// 页面标题
    pub title: Option<String>,
    /// 提取的正文文本（可选）
    pub content: Option<String>,
    /// HTTP 状态码
    pub status_code: u16,
    /// 页面深度（从种子 URL 开始算）
    pub depth: u32,
    /// 提取到的所有链接
    pub links: Vec<Link>,
    /// 抓取耗时
    pub fetch_duration_ms: u64,
    /// 抓取时间戳
    pub crawled_at: SystemTime,
}

/// 链接信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    /// 链接文本
    pub text: String,
    /// 链接 URL（绝对 URL）
    pub url: String,
    /// 是否属于同一域名
    pub is_internal: bool,
}

/// 爬取统计
#[derive(Debug, Default)]
pub struct CrawlStats {
    /// 已请求页面数
    pub pages_fetched: usize,
    /// 成功解析页面数
    pub pages_parsed: usize,
    /// 失败请求数
    pub pages_failed: usize,
    /// 发现的新链接数
    pub links_discovered: usize,
    /// 实际排队去重后的链接数
    pub links_queued: usize,
    /// 总抓取耗时
    pub total_duration: Duration,
}

/// 爬取任务（在队列中传递）
#[derive(Debug, Clone)]
pub struct CrawlTask {
    pub url: Url,
    pub depth: u32,
}

/// 爬取结果（在结果通道中传递）
pub enum CrawlResult {
    Success(Page),
    Failed { url: String, error: String, depth: u32 },
}
```

### 3.2 配置结构 (`src/config.rs`)

```rust
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CrawlerConfig {
    /// 种子 URL 列表
    pub seeds: Vec<String>,
    /// 最大爬取深度
    pub max_depth: u32,
    /// 最大并发请求数
    pub max_concurrency: usize,
    /// 请求超时（秒）
    pub timeout_secs: u64,
    /// 请求间隔（毫秒，礼貌爬取）
    pub delay_ms: u64,
    /// 允许爬取的域名白名单（空表示不限）
    pub allowed_domains: Vec<String>,
    /// 排除的 URL 模式（正则或子串）
    pub exclude_patterns: Vec<String>,
    /// 用户代理字符串
    pub user_agent: String,
    /// 最大爬取页面数（0 表示不限）
    pub max_pages: usize,
    /// 输出格式
    pub output_format: OutputFormat,
    /// 输出文件路径
    pub output_path: PathBuf,
    /// 是否遵循 robots.txt
    pub respect_robots_txt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Csv,
    Sqlite,
}
```

---

## 4. 模块详细设计

### 4.1 配置模块 (`config.rs`)

**职责**：加载和验证配置（CLI 参数 + 默认配置）

**关键函数**：
```rust
impl CrawlerConfig {
    /// 从命令行参数构建配置
    pub fn from_cli() -> Result<Self, ConfigError>;
    
    /// 验证配置合法性
    pub fn validate(&self) -> Result<(), ConfigError>;
    
    /// 检查 URL 是否在允许范围内
    pub fn is_url_allowed(&self, url: &Url) -> bool;
}
```

### 4.2 URL 去重模块 (`deduper.rs`)

**职责**：确保同一 URL 不会被重复抓取

**设计选择**：
- 使用内存中的 `HashSet<String>`（单机场景足够）
- 包装在 `RwLock` 中支持并发读、独占写
- 可选：使用 BloomFilter 节省内存（加分项）

```rust
use std::collections::HashSet;
use tokio::sync::RwLock;

pub struct UrlDeduper {
    seen: RwLock<HashSet<String>>,
    total_seen: std::sync::atomic::AtomicUsize,
}

impl UrlDeduper {
    pub fn new() -> Self;
    
    /// 尝试将 URL 加入已抓取集合
    /// 返回 true 表示是新 URL，false 表示已存在
    pub async fn try_add(&self, url: &str) -> bool;
    
    /// 获取已去重 URL 数量
    pub fn len(&self) -> usize;
}
```

### 4.3 过滤模块 (`filter.rs`)

**职责**：决定是否应该抓取某个 URL

```rust
pub struct UrlFilter {
    config: CrawlerConfig,
}

impl UrlFilter {
    pub fn new(config: CrawlerConfig) -> Self;
    
    /// 综合判断：URL 是否应该被抓取
    pub fn should_crawl(&self, url: &Url, depth: u32) -> FilterResult;
}

pub enum FilterResult {
    Allow,
    Deny(&'static str), // 拒绝原因
}
```

### 4.4 HTTP 获取模块 (`fetcher.rs`)

**职责**：发送 HTTP 请求，获取页面内容

```rust
use reqwest::Client;

pub struct Fetcher {
    client: Client,
    config: CrawlerConfig,
}

/// 原始获取结果
pub struct FetchResult {
    pub url: String,
    pub status: u16,
    pub headers: reqwest::header::HeaderMap,
    pub body: String,
    pub duration_ms: u64,
}

impl Fetcher {
    pub fn new(config: &CrawlerConfig) -> Result<Self, FetcherError>;
    
    /// 异步获取单个 URL
    pub async fn fetch(&self, url: &str) -> Result<FetchResult, FetcherError>;
}
```

**错误类型**：
```rust
#[derive(Debug, thiserror::Error)]
pub enum FetcherError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("URL parse error: {0}")]
    UrlError(#[from] url::ParseError),
    #[error("Timeout after {0}s")]
    Timeout(u64),
    #[error("Invalid status: {0}")]
    BadStatus(u16),
}
```

### 4.5 HTML 解析模块 (`parser.rs`)

**职责**：解析 HTML，提取标题、正文、链接

```rust
use scraper::{Html, Selector};
use url::Url;

pub struct HtmlParser;

impl HtmlParser {
    /// 解析 HTML，提取 Page 结构
    pub fn parse(base_url: &Url, html: &str, depth: u32) -> Page;
    
    /// 提取所有链接并转为绝对 URL
    fn extract_links(base_url: &Url, document: &Html) -> Vec<Link>;
    
    /// 提取页面标题
    fn extract_title(document: &Html) -> Option<String>;
    
    /// 提取正文内容（去除脚本/样式）
    fn extract_content(document: &Html) -> Option<String>;
}
```

### 4.6 存储模块 (`storage.rs`）

**职责**：将爬取结果持久化到不同格式

```rust
use async_trait::async_trait;

#[async_trait]
pub trait Storage: Send + Sync {
    /// 保存单个页面
    async fn save_page(&self, page: &Page) -> Result<(), StorageError>;
    
    /// 关闭存储，刷新缓存
    async fn close(&self) -> Result<(), StorageError>;
}

/// JSON 存储实现
pub struct JsonStorage { /* ... */ }

/// CSV 存储实现
pub struct CsvStorage { /* ... */ }

/// SQLite 存储实现
pub struct SqliteStorage { /* ... */ }

/// 存储工厂
pub fn create_storage(
    format: OutputFormat,
    path: &Path
) -> Result<Box<dyn Storage>, StorageError>;
```

**为什么用 `async_trait`**：SQLite 写入可能阻塞，可以放到 `tokio::task::spawn_blocking` 中。

### 4.7 核心调度模块 (`crawler.rs`) — 最复杂、最关键

**职责**：协调所有模块，管理爬取生命周期

```rust
pub struct Crawler {
    config: CrawlerConfig,
    fetcher: Fetcher,
    deduper: UrlDeduper,
    filter: UrlFilter,
    storage: Box<dyn Storage>,
    stats: Arc<RwLock<CrawlStats>>,
}

impl Crawler {
    pub async fn new(config: CrawlerConfig) -> Result<Self, CrawlerError>;
    
    /// 启动爬取，阻塞直到完成
    pub async fn run(&self) -> Result<CrawlStats, CrawlerError>;
    
    /// 优雅的关闭（保存状态）
    pub async fn shutdown(&self) -> Result<(), CrawlerError>;
}

/// 内部：单个抓取任务
async fn crawl_task(
    task: CrawlTask,
    fetcher: Arc<Fetcher>,
    deduper: Arc<UrlDeduper>,
    filter: Arc<UrlFilter>,
    result_tx: mpsc::Sender<CrawlResult>,
    task_tx: mpsc::Sender<CrawlTask>,
    _permit: OwnedSemaphorePermit, // 信号量许可，drop 时释放
) {
    // 1. 获取页面
    // 2. 解析 HTML
    // 3. 提取新链接，过滤后加入任务队列
    // 4. 发送结果到结果通道
}
```

---

## 5. 主程序流程 (`main.rs`)

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 初始化日志和进度显示
    tracing_subscriber::fmt::init();
    
    // 2. 解析命令行参数
    let config = CrawlerConfig::from_cli()?;
    config.validate()?;
    
    // 3. 初始化进度条
    let pb = ProgressBar::new(100);
    pb.set_style(...);
    
    // 4. 创建并启动爬虫
    let crawler = Crawler::new(config).await?;
    
    // 5. 设置 Ctrl+C 处理
    let crawler_for_signal = crawler.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        println!("\n收到退出信号，正在保存数据...");
        crawler_for_signal.shutdown().await.ok();
    });
    
    // 6. 运行爬虫
    let stats = crawler.run().await?;
    
    // 7. 输出统计
    println!("\n爬取完成！");
    println!("  已抓取: {} 页", stats.pages_fetched);
    println!("  成功解析: {} 页", stats.pages_parsed);
    println!("  失败: {} 页", stats.pages_failed);
    
    Ok(())
}
```

---

## 6. 开发路线图（建议按此顺序实现）

### Phase 1: MVP（最小可行产品）— 预计 3~4 天
目标：**能跑起来，抓取一个页面并保存**

- [ ] 创建项目骨架，`Cargo.toml` 添加依赖
- [ ] 实现 `models.rs` 数据结构
- [ ] 实现 `config.rs` 基础配置（硬编码种子 URL）
- [ ] 实现 `fetcher.rs` 单 URL 获取
- [ ] 实现 `parser.rs` 基础 HTML 解析（只提取标题）
- [ ] 实现 `storage.rs` JSON 存储
- [ ] 实现 `crawler.rs` 顺序爬取（无并发）
- [ ] `main.rs` 串联所有模块

**验收标准**：`cargo run` 能抓取一个网页，输出 JSON 文件包含 URL 和标题。

### Phase 2: 并发与去重 — 预计 3~4 天
目标：**支持并发抓取和 URL 去重**

- [ ] 引入 `tokio::sync::Semaphore` 控制并发
- [ ] 实现 `deduper.rs` URL 去重
- [ ] 改造 `crawler.rs` 为生产者-消费者模型
- [ ] 实现深度控制（depth 传递）
- [ ] 实现 `filter.rs` 域名过滤

**验收标准**：`cargo run -- --seeds https://example.com --depth 2` 能并发抓取多页，无重复。

### Phase 3: 功能完善 — 预计 4~5 天
目标：**功能完整，可配置**

- [ ] `clap` 命令行参数完整实现
- [ ] `parser.rs` 完善：提取正文、所有链接
- [ ] 实现 `CsvStorage` 和 `SqliteStorage`
- [ ] 添加请求延迟（礼貌爬取）
- [ ] 添加用户代理和超时配置
- [ ] `indicatif` 进度条显示
- [ ] `tracing` 日志系统

**验收标准**：完整的 CLI 可用，支持 `--format json/csv/sqlite`。

### Phase 4: 测试与工程规范 — 预计 3~4 天
目标：**通过 cargo test / fmt / clippy**

- [ ] 单元测试：`deduper`, `filter`, `parser`
- [ ] 集成测试：用 `wiremock` 模拟 HTTP 服务器测试完整流程
- [ ] 错误处理完善（消除 unwrap）
- [ ] `cargo fmt` 格式化
- [ ] `cargo clippy` 无警告
- [ ] 编写 `README.md`

### Phase 5: 文档与演示 — 预计 2~3 天
目标：**提交就绪**

- [ ] 撰写实验报告
- [ ] 录制 5 分钟演示视频
- [ ] 最终测试和 Bug 修复

---

## 7. CLI 设计

```bash
# 基础用法：从单个种子开始，深度2，JSON输出
rust-crawler --seed https://example.com --depth 2 --output results.json

# 多种子 + 域名限制 + CSV输出
rust-crawler \
  --seed https://docs.rs \
  --seed https://crates.io \
  --allowed-domain docs.rs \
  --allowed-domain crates.io \
  --depth 3 \
  --format csv \
  --output results.csv

# SQLite + 并发控制 + 进度显示
rust-crawler \
  --seed https://rust-lang.org \
  --depth 2 \
  --concurrency 10 \
  --format sqlite \
  --output crawl.db \
  --verbose
```

---

## 8. 关键技术点详解

### 8.1 并发控制实现

```rust
use tokio::sync::Semaphore;
use std::sync::Arc;

let semaphore = Arc::new(Semaphore::new(config.max_concurrency));

// 每个任务获取一个许可
let permit = semaphore.clone().acquire_owned().await?;

// 将许可移动到任务中，任务结束时自动释放
tokio::spawn(async move {
    crawl_page(url).await;
    drop(permit); // 显式释放（或等变量离开作用域）
});
```

### 8.2 避免 unwrap 的模式

```rust
// ❌ 不推荐
let title = document.select(&selector).next().unwrap().text().collect();

// ✅ 推荐：函数式链式 + unwrap 替代
let title = document
    .select(&selector)
    .next()
    .map(|el| el.text().collect::<String>()
    .trim()
    .to_string());

// 或者显式错误处理
let Some(elem) = document.select(&selector).next() else {
    return Err(ParseError::MissingTitle);
};
```

### 8.3 泛型和 Trait 的使用（展示 Rust 特性）

```rust
// 存储后端抽象 —— 体现 trait 的威力
#[async_trait]
pub trait Storage: Send + Sync {
    async fn save_page(&self, page: &Page) -> Result<(), StorageError>;
}

// 可以写出与具体存储无关的代码
pub struct Crawler<S: Storage> {
    storage: S,
}

impl<S: Storage> Crawler<S> {
    pub async fn run(&self) -> Result<(), CrawlerError> {
        // ... 不关心是 JSON、CSV 还是 SQLite
        self.storage.save_page(&page).await?;
    }
}
```

### 8.4 生命周期示例（如需要）

```rust
// 解析器借用 HTML 字符串，不分配新内存
pub struct Parser<'a> {
    html: &'a str,
    base_url: &'a Url,
}

impl<'a> Parser<'a> {
    pub fn new(html: &'a str, base_url: &'a Url) -> Self {
        Self { html, base_url }
    }
    
    pub fn extract_links(&self) -> Vec<Link> {
        // 使用 self.html 和 self.base_url
    }
}
```

---

## 9. 测试策略

### 9.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_deduper_prevents_duplicates() {
        let deduper = UrlDeduper::new();
        assert!(deduper.try_add("https://example.com").await);
        assert!(!deduper.try_add("https://example.com").await);
        assert!(deduper.try_add("https://other.com").await);
    }

    #[test]
    fn test_filter_blocks_external_domain() {
        let config = CrawlerConfig {
            allowed_domains: vec!["example.com".to_string()],
            ..Default::default()
        };
        let filter = UrlFilter::new(config);
        
        let internal = Url::parse("https://example.com/page").unwrap();
        let external = Url::parse("https://other.com/page").unwrap();
        
        assert!(filter.should_crawl(&internal, 1).is_allowed());
        assert!(!filter.should_crawl(&external, 1).is_allowed());
    }
}
```

### 9.2 集成测试

使用 `wiremock` 创建模拟 HTTP 服务器：

```rust
#[tokio::test]
async fn test_full_crawl_flow() {
    // 启动 mock 服务器
    let mock_server = MockServer::start().await;
    
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_string(r#"<html><body><a href="/page2">Link</a></body></html>"#))
        .mount(&mock_server)
        .await;
    
    // 运行爬虫并验证结果
    let config = CrawlerConfig {
        seeds: vec![mock_server.uri()],
        max_depth: 1,
        ..Default::default()
    };
    
    let crawler = Crawler::new(config).await.unwrap();
    let stats = crawler.run().await.unwrap();
    
    assert_eq!(stats.pages_fetched, 1);
}
```

---

## 10. 评分点对照检查表

| 评分项 | 占比 | 本项目的体现 | 自查 |
|--------|------|-------------|------|
| 功能完整性 | 30% | 种子抓取→解析→提取→去重→存储→进度显示，完整闭环 | ☐ |
| Rust 特性使用 | 10% | ownership/borrowing, struct/enum, trait, 泛型, 生命周期, Result | ☐ |
| 工程结构 | 15% | 6+ 模块，清晰分层，接口抽象 | ☐ |
| 实验报告 | 40% | 按模板撰写 | ☐ |
| 创新性 | 5% | 可选：BloomFilter、robots.txt、TUI展示 | ☐ |

**额外加分项建议**（不必须，但能增加亮点）：
- [ ] 实现 `robots.txt` 解析和遵守
- [ ] 使用 BloomFilter 替代 HashSet 做 URL 去重
- [ ] 添加简单的 TUI 实时展示（`ratatui`）
- [ ] 支持增量爬取（记录已抓取状态）
- [ ] 添加代理支持

---

## 11. 参考资源

- `tokio` 官方文档：https://tokio.rs/
- `reqwest` 文档：https://docs.rs/reqwest
- `scraper` 文档：https://docs.rs/scraper
- `clap` 文档：https://docs.rs/clap
- `indicatif` 示例：https://github.com/console-rs/indicatif

---

> 文档版本：v1.0
> 建议：先按 Phase 1 搭建 MVP，跑通后再逐步叠加功能。不要一开始就写完整架构！
