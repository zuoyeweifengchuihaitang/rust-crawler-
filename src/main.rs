//! CLI 入口
//!
//! 程序的主入口，负责解析命令行参数、初始化日志、启动爬虫。

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use rust_crawler::{config::CrawlerConfig, Crawler};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 初始化日志系统
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
        )
        .init();

    // 2. 解析命令行参数
    let config = CrawlerConfig::parse();

    // 3. 验证配置
    if let Err(e) = config.validate() {
        eprintln!("配置错误: {}", e);
        std::process::exit(1);
    }

    // 4. 设置日志级别
    if config.verbose {
        // 已在环境变量中设置，这里可以调整
        info!("启用详细日志模式");
    }

    // 5. 打印配置信息
    print_config(&config);

    // 6. 初始化进度条
    let pb = if config.verbose {
        None
    } else {
        Some(create_progress_bar())
    };

    // 7. 创建爬虫实例
    let crawler = match Crawler::new(config.clone()).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("初始化爬虫失败: {}", e);
            std::process::exit(1);
        }
    };

    // 8. 设置 Ctrl+C 信号处理
    let crawler_for_signal = std::sync::Arc::new(crawler);
    let crawler_for_run = std::sync::Arc::clone(&crawler_for_signal);

    let signal_handle = tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            warn!("注册信号处理失败: {}", e);
            return;
        }
        println!("\n\n收到退出信号，正在保存数据...");
        if let Err(e) = crawler_for_signal.shutdown().await {
            warn!("关闭爬虫时出错: {}", e);
        }
    });

    // 9. 运行爬虫
    info!("开始爬取...");
    let start_time = std::time::Instant::now();

    let result = crawler_for_run.run().await;

    // 取消信号处理（如果爬虫正常完成）
    signal_handle.abort();

    // 10. 处理结果
    match result {
        Ok(stats) => {
            if let Some(pb) = pb {
                pb.finish_with_message("爬取完成");
            }

            let duration = start_time.elapsed();

            println!("\n{}", "=".repeat(50));
            println!("✅ 爬取完成！");
            println!("{}", "=".repeat(50));
            println!("📊 统计信息:");
            println!("   已抓取页面: {}", stats.pages_fetched);
            println!("   成功解析:   {}", stats.pages_parsed);
            println!("   失败:       {}", stats.pages_failed);
            println!(
                "   成功率:     {:.1}%",
                stats.success_rate()
            );
            println!(
                "   发现链接:   {}",
                stats.links_discovered
            );
            println!(
                "   总耗时:     {:.2} 秒",
                duration.as_secs_f64()
            );
            println!(
                "   平均速度:   {:.2} 页/秒",
                stats.pages_per_second()
            );
            println!();
            println!(
                "💾 数据已保存至: {}",
                config.output_path_with_ext().display()
            );
        }
        Err(e) => {
            if let Some(pb) = pb {
                pb.finish_with_message("爬取失败");
            }
            eprintln!("❌ 爬取失败: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

/// 打印配置信息
fn print_config(config: &CrawlerConfig) {
    println!("{}", "=".repeat(50));
    println!("🕷️  Rust Crawler 配置");
    println!("{}", "=".repeat(50));
    println!("种子 URL:    {}", config.seeds.join(", "));
    println!("最大深度:    {}", config.max_depth);
    println!("并发数:      {}", config.max_concurrency);
    println!("超时:        {} 秒", config.timeout_secs);
    println!("请求间隔:    {} 毫秒", config.delay_ms);
    println!("输出格式:    {}", config.format);
    println!(
        "输出路径:    {}",
        config.output_path_with_ext().display()
    );

    if !config.allowed_domains.is_empty() {
        println!(
            "允许域名:    {}",
            config.allowed_domains.join(", ")
        );
    }

    if config.max_pages > 0 {
        println!("最大页面数:  {}", config.max_pages);
    }

    if config.respect_robots {
        println!("遵循 robots.txt: 是");
    }

    println!("{}", "=".repeat(50));
    println!();
}

/// 创建进度条
fn create_progress_bar() -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} [{elapsed_precise}] {msg}")
            .expect("无效的进度条模板")
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
    );
    pb.set_message("正在爬取...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}
