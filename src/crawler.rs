//! 核心调度模块
//!
//! 协调所有模块，管理爬取生命周期。
//! 使用 tokio 异步运行时实现并发抓取。

use crate::{
    config::CrawlerConfig,
    deduper::{create_deduper, UrlDeduper},
    fetcher::{Fetcher, FetcherError},
    filter::{FilterResult, UrlFilter},
    models::{CrawlResult, CrawlStats, CrawlTask},
    parser::HtmlParser,
    storage::{create_storage, Storage},
};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// 爬虫错误类型
#[derive(Debug, thiserror::Error)]
pub enum CrawlerError {
    #[error("获取器初始化失败: {0}")]
    FetcherInit(#[from] FetcherError),

    #[error("存储初始化失败: {0}")]
    StorageInit(#[from] crate::storage::StorageError),

    #[error("URL 解析错误: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("任务发送失败")]
    TaskSendError,

    #[error("结果发送失败")]
    ResultSendError,

    #[error("爬虫被中断")]
    Interrupted,
}

/// 爬虫核心调度器
///
/// 负责协调抓取、解析、存储等所有操作。
pub struct Crawler {
    config: CrawlerConfig,
    fetcher: Arc<Fetcher>,
    deduper: Arc<dyn UrlDeduper>,
    filter: Arc<UrlFilter>,
    storage: Arc<dyn Storage>,
    stats: Arc<RwLock<CrawlStats>>,
    #[allow(dead_code)]
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl Crawler {
    /// 创建新的爬虫实例
    ///
    /// 初始化所有子模块（获取器、去重器、过滤器、存储）。
    pub async fn new(config: CrawlerConfig) -> Result<Self, CrawlerError> {
        // 初始化 HTTP 获取器
        let fetcher = Arc::new(Fetcher::new(
            &config.user_agent,
            config.timeout_secs,
        )?);

        // 初始化 URL 去重器
        let deduper: Arc<dyn UrlDeduper> = Arc::from(create_deduper());

        // 初始化 URL 过滤器
        let filter = Arc::new(UrlFilter::new(config.clone()));

        // 初始化存储
        let output_path = config.output_path_with_ext();
        let storage: Arc<dyn Storage> =
            Arc::from(create_storage(config.format, &output_path)?);

        // 初始化统计
        let stats = Arc::new(RwLock::new(CrawlStats::default()));

        Ok(Self {
            config,
            fetcher,
            deduper,
            filter,
            storage,
            stats,
            shutdown_tx: None,
        })
    }

    /// 启动爬取，阻塞直到完成
    ///
    /// 这是爬虫的核心方法，协调所有模块完成爬取任务。
    pub async fn run(&self) -> Result<CrawlStats, CrawlerError> {
        let start_time = std::time::Instant::now();

        // 创建任务队列和结果队列
        let (task_tx, mut task_rx) = mpsc::channel::<CrawlTask>(1000);
        let (result_tx, result_rx) = mpsc::channel::<CrawlResult>(1000);

        // 将种子 URL 加入任务队列
        for seed in &self.config.seeds {
            let url = url::Url::parse(seed)?;
            let task = CrawlTask::new(url, 0);

            // 同时加入去重器
            self.deduper.try_add(seed).await;

            task_tx
                .send(task)
                .await
                .map_err(|_| CrawlerError::TaskSendError)?;
        }

        // 启动结果处理任务（往存储写入）和新链接处理
        let storage_clone = Arc::clone(&self.storage);
        let stats_clone = Arc::clone(&self.stats);
        let deduper_clone = Arc::clone(&self.deduper);
        let filter_clone = Arc::clone(&self.filter);

        let result_handle = {
            let task_tx = task_tx.clone();
            tokio::spawn(async move {
                let mut result_rx = result_rx;
                // 结果消费循环：处理已抓取的页面
                while let Some(result) = result_rx.recv().await {
                    match result {
                        CrawlResult::Success(page) => {
                            // 保存页面到存储
                            if let Err(e) = storage_clone.save_page(&page).await {
                                tracing::warn!("保存页面失败: {}", e);
                            }

                            // 更新统计
                            {
                                let mut stats = stats_clone.write().await;
                                stats.pages_parsed += 1;
                                stats.links_discovered += page.links.len();
                            }

                            // 处理新发现的链接（核心逻辑：链接提取和去重）
                            for link in page.links.iter() {
                                // 判断是否应该继续爬取这个链接
                                if let Ok(link_url) = url::Url::parse(&link.url) {
                                    // 应用过滤策略（深度、域名等）
                                    if filter_clone
                                        .should_crawl(&link_url, page.depth + 1)
                                        .is_allowed()
                                    {
                                        // 尝试加入去重表，如果是新 URL
                                        if deduper_clone.try_add(&link.url).await {
                                            // 创建新任务
                                            let new_task =
                                                CrawlTask::new(link_url, page.depth + 1);

                                            // 发送到任务队列
                                            if let Err(e) = task_tx.send(new_task).await {
                                                tracing::debug!(
                                                    "任务入队失败（队列可能已满）: {}",
                                                    e
                                                );
                                                break;
                                            }

                                            // 更新入队统计
                                            {
                                                let mut stats = stats_clone.write().await;
                                                stats.links_queued += 1;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        CrawlResult::Failed { url, error, .. } => {
                            tracing::debug!("抓取失败 {}: {}", url, error);
                            let mut stats = stats_clone.write().await;
                            stats.pages_failed += 1;
                        }
                    }
                }
            })
        };

        // 使用信号量限制并发数
        let semaphore = Arc::new(tokio::sync::Semaphore::new(
            self.config.max_concurrency,
        ));

        // 主循环：处理任务队列，生成抓取任务
        let mut task_count = 0;
        while let Ok(permit) = semaphore.clone().acquire_owned().await {
            // 检查是否达到最大页面数限制
            if self.config.max_pages > 0 {
                let stats = self.stats.read().await;
                if stats.pages_fetched >= self.config.max_pages {
                    drop(permit); // 释放许可
                    break;
                }
            }

            // 从任务队列取任务
            let task = match task_rx.recv().await {
                Some(t) => t,
                None => {
                    // 任务队列已关闭，没有更多任务
                    drop(permit);
                    break;
                }
            };

            task_count += 1;

            // 克隆必要的 Arc 供异步任务使用
            let fetcher = Arc::clone(&self.fetcher);
            let filter = Arc::clone(&self.filter);
            let result_tx = result_tx.clone();
            let stats = Arc::clone(&self.stats);

            // 启动抓取任务
            tokio::spawn(async move {
                // 更新已抓取计数
                {
                    let mut s = stats.write().await;
                    s.pages_fetched += 1;
                }

                // 执行抓取
                let crawl_result =
                    Self::process_task(task, fetcher, filter).await;

                // 发送结果到结果通道
                let _ = result_tx.send(crawl_result).await;

                // 释放信号量许可（drop permit）
                drop(permit);
            });

            // 请求延迟（礼貌爬取）
            if self.config.delay_ms > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(
                    self.config.delay_ms,
                ))
                .await;
            }

            // 调试信息
            if task_count % 10 == 0 {
                tracing::debug!("已处理 {} 个任务", task_count);
            }
        }

        // 关闭任务发送端，等待所有任务完成
        drop(task_tx);

        // 关闭结果发送端
        drop(result_tx);

        // 等待结果处理任务完成
        let _ = result_handle.await;

        // 更新总耗时
        let total_duration = start_time.elapsed();
        {
            let mut stats = self.stats.write().await;
            stats.total_duration = total_duration;
        }

        // 关闭存储
        self.storage.close().await?;

        // 返回最终统计
        let final_stats = self.stats.read().await.clone();
        tracing::info!(
            "爬虫完成：抓取 {} 页，成功 {} 页，失败 {} 页，耗时 {:.2}s",
            final_stats.pages_fetched,
            final_stats.pages_parsed,
            final_stats.pages_failed,
            total_duration.as_secs_f64()
        );
        Ok(final_stats)
    }

    /// 处理单个爬取任务
    ///
    /// 抓取页面、解析 HTML、提取新链接。
    async fn process_task(
        task: CrawlTask,
        fetcher: Arc<Fetcher>,
        filter: Arc<UrlFilter>,
    ) -> CrawlResult {
        // 过滤检查
        match filter.should_crawl(&task.url, task.depth) {
            FilterResult::Allow => {}
            FilterResult::Deny(reason) => {
                return CrawlResult::Failed {
                    url: task.url.to_string(),
                    error: reason.to_string(),
                    depth: task.depth,
                };
            }
        }

        // 发送 HTTP 请求
        let fetch_result = match fetcher.fetch(task.url.as_str()).await {
            Ok(result) => result,
            Err(e) => {
                return CrawlResult::Failed {
                    url: task.url.to_string(),
                    error: e.to_string(),
                    depth: task.depth,
                };
            }
        };

        // 解析 HTML
        let page = HtmlParser::parse(
            &task.url,
            &fetch_result.body,
            task.depth,
            fetch_result.status.as_u16(),
            fetch_result.duration_ms,
        );

        CrawlResult::Success(page)
    }

    /// 优雅关闭爬虫
    ///
    /// 保存当前状态，关闭存储。
    pub async fn shutdown(&self) -> Result<(), CrawlerError> {
        tracing::info!("正在关闭爬虫...");
        self.storage.close().await?;
        tracing::info!("爬虫已关闭");
        Ok(())
    }

    /// 获取当前统计信息
    pub async fn stats(&self) -> CrawlStats {
        self.stats.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CrawlerConfig;

    // 注意：这些测试需要异步运行时

    #[tokio::test]
    async fn test_crawler_creation() {
        let config = CrawlerConfig::default_for_test();
        let crawler = Crawler::new(config).await;
        assert!(crawler.is_ok());
    }
}
