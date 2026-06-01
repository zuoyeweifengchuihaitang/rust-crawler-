//! rust-crawler - 一个基于异步运行时的高性能网页爬虫
//!
//! 本项目是 Rust 程序设计课程期末作业，展示了 Rust 的核心特性：
//! - 所有权与借用系统
//! - 泛型与 trait 抽象
//! - 异步编程 (tokio)
//! - 错误处理 (Result)
//! - 模块化设计

pub mod config;
pub mod crawler;
pub mod deduper;
pub mod fetcher;
pub mod filter;
pub mod models;
pub mod parser;
pub mod storage;

// Re-export 核心类型，方便外部使用
pub use config::{CrawlerConfig, OutputFormat};
pub use crawler::Crawler;
pub use models::{CrawlResult, CrawlStats, CrawlTask, Link, Page};
