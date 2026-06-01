//! 核心数据模型定义
//!
//! 包含爬虫系统中使用的所有数据结构：页面、链接、任务、结果、统计等。

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};
use url::Url;

/// 爬取到的页面数据
///
/// 这是系统的核心数据结构，表示一次成功的页面抓取结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Page {
    /// 页面 URL（规范化后的绝对 URL）
    pub url: String,
    /// 页面标题
    pub title: Option<String>,
    /// 提取的正文文本（去除 HTML 标签后的纯文本）
    pub content: Option<String>,
    /// HTTP 状态码
    pub status_code: u16,
    /// 页面深度（从种子 URL 开始算，种子深度为 0）
    pub depth: u32,
    /// 提取到的所有链接
    pub links: Vec<Link>,
    /// 抓取耗时（毫秒）
    pub fetch_duration_ms: u64,
    /// 抓取时间戳
    #[serde(with = "serde_timestamp")]
    pub crawled_at: SystemTime,
}

impl Page {
    /// 创建一个新的 Page 实例
    pub fn new(
        url: String,
        status_code: u16,
        depth: u32,
        fetch_duration_ms: u64,
    ) -> Self {
        Self {
            url,
            title: None,
            content: None,
            status_code,
            depth,
            links: Vec::new(),
            fetch_duration_ms,
            crawled_at: SystemTime::now(),
        }
    }

    /// 获取内部链接数量
    pub fn internal_link_count(&self) -> usize {
        self.links.iter().filter(|l| l.is_internal).count()
    }

    /// 获取外部链接数量
    pub fn external_link_count(&self) -> usize {
        self.links.iter().filter(|l| !l.is_internal).count()
    }
}

/// 链接信息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Link {
    /// 链接文本（a 标签内的文本内容）
    pub text: String,
    /// 链接 URL（绝对 URL）
    pub url: String,
    /// 是否属于同一域名（相对于来源页面）
    pub is_internal: bool,
}

impl Link {
    pub fn new(text: String, url: String, is_internal: bool) -> Self {
        Self {
            text,
            url,
            is_internal,
        }
    }
}

/// 爬取任务（在任务队列中传递）
#[derive(Debug, Clone)]
pub struct CrawlTask {
    /// 要抓取的 URL
    pub url: Url,
    /// 当前深度
    pub depth: u32,
}

impl CrawlTask {
    pub fn new(url: Url, depth: u32) -> Self {
        Self { url, depth }
    }
}

/// 爬取结果（在结果通道中传递）
pub enum CrawlResult {
    /// 成功抓取并解析
    Success(Page),
    /// 抓取失败
    Failed {
        /// 失败的 URL
        url: String,
        /// 错误信息
        error: String,
        /// 尝试时的深度
        depth: u32,
    },
}

/// 爬取统计
#[derive(Debug, Default, Clone)]
pub struct CrawlStats {
    /// 已请求页面数
    pub pages_fetched: usize,
    /// 成功解析页面数
    pub pages_parsed: usize,
    /// 失败请求数
    pub pages_failed: usize,
    /// 发现的新链接数（去重前）
    pub links_discovered: usize,
    /// 实际排队去重后的链接数
    pub links_queued: usize,
    /// 总抓取耗时
    pub total_duration: Duration,
}

impl CrawlStats {
    /// 成功率百分比
    pub fn success_rate(&self) -> f64 {
        if self.pages_fetched == 0 {
            0.0
        } else {
            (self.pages_parsed as f64 / self.pages_fetched as f64) * 100.0
        }
    }

    /// 每秒抓取页面数
    pub fn pages_per_second(&self) -> f64 {
        let secs = self.total_duration.as_secs_f64();
        if secs > 0.0 {
            self.pages_fetched as f64 / secs
        } else {
            0.0
        }
    }
}

/// SystemTime 的序列化辅助模块
mod serde_timestamp {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let duration = time.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
        serializer.serialize_u64(duration.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + Duration::from_secs(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_creation() {
        let page = Page::new(
            "https://example.com".to_string(),
            200,
            0,
            100,
        );
        assert_eq!(page.url, "https://example.com");
        assert_eq!(page.status_code, 200);
        assert_eq!(page.depth, 0);
        assert!(page.title.is_none());
    }

    #[test]
    fn test_link_counts() {
        let mut page = Page::new("https://example.com".to_string(), 200, 0, 100);
        page.links.push(Link::new(
            "Home".to_string(),
            "https://example.com/home".to_string(),
            true,
        ));
        page.links.push(Link::new(
            "External".to_string(),
            "https://other.com".to_string(),
            false,
        ));

        assert_eq!(page.internal_link_count(), 1);
        assert_eq!(page.external_link_count(), 1);
    }

    #[test]
    fn test_stats_success_rate() {
        let mut stats = CrawlStats::default();
        assert_eq!(stats.success_rate(), 0.0);

        stats.pages_fetched = 10;
        stats.pages_parsed = 8;
        assert_eq!(stats.success_rate(), 80.0);
    }
}
