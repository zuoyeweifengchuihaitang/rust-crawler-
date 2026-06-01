//! 集成测试
//!
//! 使用 wiremock 创建模拟 HTTP 服务器，测试完整的爬取流程。

#![allow(unused)]

use rust_crawler::{config::CrawlerConfig, Crawler};
use std::time::Duration;

// TODO: 添加 wiremock 集成测试
// 需要启动一个模拟服务器，然后让爬虫抓取

/// 创建基本的测试配置
fn create_test_config(base_url: &str) -> CrawlerConfig {
    CrawlerConfig {
        seeds: vec![base_url.to_string()],
        max_depth: 2,
        max_concurrency: 2,
        timeout_secs: 5,
        delay_ms: 0,
        allowed_domains: vec![],
        exclude_patterns: vec![],
        user_agent: "rust-crawler/test".to_string(),
        max_pages: 5,
        format: rust_crawler::config::OutputFormat::Json,
        output: std::path::PathBuf::from("test_output"),
        respect_robots: false,
        verbose: true,
    }
}

#[tokio::test]
async fn test_crawler_with_mock_server() {
    // TODO: 使用 wiremock 启动模拟服务器
    // 1. 设置 mock 响应
    // 2. 创建爬虫
    // 3. 运行爬虫
    // 4. 验证结果
}

#[tokio::test]
async fn test_crawler_respects_max_depth() {
    // TODO: 测试深度限制是否生效
}

#[tokio::test]
async fn test_crawler_respects_max_pages() {
    // TODO: 测试最大页面数限制是否生效
}

#[tokio::test]
async fn test_crawler_domain_filter() {
    // TODO: 测试域名过滤是否生效
}
