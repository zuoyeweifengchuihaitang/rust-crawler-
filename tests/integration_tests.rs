//! 集成测试
//!
//! 使用 wiremock 创建模拟 HTTP 服务器，测试完整的爬取流程。

#![allow(unused)]

use rust_crawler::{config::CrawlerConfig, Crawler};
use std::time::Duration;
use wiremock::{matchers::*, Mock, MockServer, ResponseTemplate};

/// 创建基本的测试配置
fn create_test_config(base_url: &str) -> CrawlerConfig {
    let host = url::Url::parse(base_url)
        .unwrap()
        .host_str()
        .unwrap()
        .to_string();

    CrawlerConfig {
        seeds: vec![base_url.to_string()],
        max_depth: 2,
        max_concurrency: 2,
        timeout_secs: 5,
        retry_count: 2,
        retry_delay_ms: 100,
        delay_ms: 0,
        allowed_domains: vec![host],
        exclude_patterns: vec![],
        user_agent: "rust-crawler/test".to_string(),
        max_pages: 0,
        format: rust_crawler::config::OutputFormat::Json,
        output: std::path::PathBuf::from("test_output"),
        respect_robots: false,
        verbose: false,
    }
}

#[tokio::test]
async fn test_crawler_with_mock_server() {
    // 启动模拟服务器
    let mock_server = MockServer::start().await;

    // 首页：包含两个链接
    Mock::given(method("GET"))
        .and(path_regex("^/?$"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"<html><head><title>Home Page</title></head>
            <body>
                <h1>Welcome</h1>
                <a href="/about">About</a>
                <a href="/contact">Contact</a>
            </body></html>
            "#,
            "text/html; charset=utf-8",
        ))
        .mount(&mock_server)
        .await;

    // About 页面：包含一个链接回到首页
    Mock::given(method("GET"))
        .and(path("/about"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"
            <html><head><title>About Page</title></head>
            <body>
                <h1>About Us</h1>
                <a href="/">Back to Home</a>
            </body></html>
            "#,
            "text/html; charset=utf-8",
        ))
        .mount(&mock_server)
        .await;

    // Contact 页面：无链接
    Mock::given(method("GET"))
        .and(path("/contact"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"
            <html><head><title>Contact Page</title></head>
            <body>
                <h1>Contact Us</h1>
            </body></html>
            "#,
            "text/html; charset=utf-8",
        ))
        .mount(&mock_server)
        .await;

    // 配置爬虫
    let config = CrawlerConfig {
        seeds: vec![mock_server.uri()],
        max_depth: 2,
        max_concurrency: 2,
        timeout_secs: 5,
        retry_count: 2,
        retry_delay_ms: 100,
        delay_ms: 0,
        allowed_domains: vec![],
        exclude_patterns: vec![],
        user_agent: "rust-crawler/test".to_string(),
        max_pages: 0,
        format: rust_crawler::config::OutputFormat::Json,
        output: std::path::PathBuf::from("test_integration_output"),
        respect_robots: false,
        verbose: false,
    };

    // 创建并运行爬虫
    let crawler = Crawler::new(config).await.unwrap();
    let stats = crawler.run().await.unwrap();

    // 验证：应该抓到 3 个页面（首页、about、contact）
    // 但首页的链接 about 和 contact 会被抓取
    // about 的链接 / 是首页，已经被去重，不会再抓取
    assert_eq!(stats.pages_fetched, 3, "应该抓到 3 个页面");
    assert_eq!(stats.pages_parsed, 3, "应该解析 3 个页面");

    // 验证输出文件存在
    assert!(
        std::path::Path::new("test_integration_output.json").exists(),
        "输出文件应该存在"
    );
}

#[tokio::test]
async fn test_crawler_respects_max_depth() {
    let mock_server = MockServer::start().await;

    // 首页 → 链接到 level1
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"<html><head><title>Level 0</title></head>
            <body><a href="/level1">Level 1</a></body></html>
            "#,
            "text/html; charset=utf-8",
        ))
        .mount(&mock_server)
        .await;

    // Level 1 → 链接到 level2
    Mock::given(method("GET"))
        .and(path("/level1"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"
            <html><head><title>Level 1</title></head>
            <body><a href="/level2">Level 2</a></body></html>
            "#,
            "text/html; charset=utf-8",
        ))
        .mount(&mock_server)
        .await;

    // Level 2
    Mock::given(method("GET"))
        .and(path("/level2"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"
            <html><head><title>Level 2</title></head>
            <body><h1>Deep Page</h1></body></html>
            "#,
            "text/html; charset=utf-8",
        ))
        .mount(&mock_server)
        .await;

    // 最大深度 1：只抓首页和 level1
    let config = CrawlerConfig {
        seeds: vec![mock_server.uri()],
        max_depth: 1,
        max_concurrency: 2,
        timeout_secs: 5,
        retry_count: 2,
        retry_delay_ms: 100,
        delay_ms: 0,
        allowed_domains: vec![],
        exclude_patterns: vec![],
        user_agent: "rust-crawler/test".to_string(),
        max_pages: 0,
        format: rust_crawler::config::OutputFormat::Json,
        output: std::path::PathBuf::from("test_depth_output"),
        respect_robots: false,
        verbose: false,
    };

    let crawler = Crawler::new(config).await.unwrap();
    let stats = crawler.run().await.unwrap();

    // 深度 1：只抓到首页（深度0）和 level1（深度1），level2（深度2）被过滤
    assert_eq!(stats.pages_fetched, 2, "深度1应该只抓2个页面");
}

#[tokio::test]
async fn test_crawler_respects_max_pages() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"
            <html><head><title>Home</title></head>
            <body>
                <a href="/page1">Page 1</a>
                <a href="/page2">Page 2</a>
                <a href="/page3">Page 3</a>
            </body></html>
            "#,
            "text/html; charset=utf-8",
        ))
        .mount(&mock_server)
        .await;

    // 子页面
    for i in 1..=3 {
        Mock::given(method("GET"))
            .and(path(format!("/page{}", i)))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                format!(
                    r#"
                <html><head><title>Page {}</title></head>
                <body><h1>Page {}</h1></body></html>
                "#,
                    i, i
                ),
                "text/html; charset=utf-8",
            ))
            .mount(&mock_server)
            .await;
    }

    // 最大页面数 2
    let config = CrawlerConfig {
        seeds: vec![mock_server.uri()],
        max_depth: 2,
        max_concurrency: 2,
        timeout_secs: 5,
        retry_count: 2,
        retry_delay_ms: 100,
        delay_ms: 0,
        allowed_domains: vec![],
        exclude_patterns: vec![],
        user_agent: "rust-crawler/test".to_string(),
        max_pages: 2,
        format: rust_crawler::config::OutputFormat::Json,
        output: std::path::PathBuf::from("test_pages_output"),
        respect_robots: false,
        verbose: false,
    };

    let crawler = Crawler::new(config).await.unwrap();
    let stats = crawler.run().await.unwrap();

    assert_eq!(stats.pages_fetched, 2, "max_pages=2 应该只抓2个页面");
}

#[tokio::test]
async fn test_crawler_domain_filter() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"
            <html><head><title>Home</title></head>
            <body>
                <a href="/internal">Internal</a>
                <a href="https://other.com/external">External</a>
            </body></html>
            "#,
            "text/html; charset=utf-8",
        ))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/internal"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"
            <html><head><title>Internal</title></head>
            <body><h1>Internal Page</h1></body></html>
            "#,
            "text/html; charset=utf-8",
        ))
        .mount(&mock_server)
        .await;

    // 设置域名白名单，只允许 mock_server 的域名
    let allowed = mock_server
        .uri()
        .parse::<url::Url>()
        .unwrap()
        .host_str()
        .unwrap()
        .to_string();

    let config = CrawlerConfig {
        seeds: vec![mock_server.uri()],
        max_depth: 2,
        max_concurrency: 2,
        timeout_secs: 5,
        retry_count: 2,
        retry_delay_ms: 100,
        delay_ms: 0,
        allowed_domains: vec![allowed],
        exclude_patterns: vec![],
        user_agent: "rust-crawler/test".to_string(),
        max_pages: 0,
        format: rust_crawler::config::OutputFormat::Json,
        output: std::path::PathBuf::from("test_domain_output"),
        respect_robots: false,
        verbose: false,
    };

    let crawler = Crawler::new(config).await.unwrap();
    let stats = crawler.run().await.unwrap();

    // 外部链接被过滤，只抓首页和 internal
    assert_eq!(
        stats.pages_fetched, 2,
        "域名过滤应该只抓2个页面（外部链接被过滤）"
    );
}
