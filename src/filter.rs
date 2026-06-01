//! URL 过滤模块
//!
//! 决定是否应该抓取某个 URL，基于深度、域名、排除模式等规则。

use crate::config::CrawlerConfig;
use url::Url;

/// 过滤结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterResult {
    /// 允许抓取
    Allow,
    /// 拒绝抓取，附带原因
    Deny(&'static str),
}

impl FilterResult {
    /// 检查是否允许
    pub fn is_allowed(&self) -> bool {
        matches!(self, FilterResult::Allow)
    }

    /// 获取拒绝原因（如果是拒绝的话）
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            FilterResult::Allow => None,
            FilterResult::Deny(reason) => Some(reason),
        }
    }
}

/// URL 过滤器
///
/// 综合判断 URL 是否应该被抓取。
pub struct UrlFilter {
    config: CrawlerConfig,
}

impl UrlFilter {
    /// 创建新的 URL 过滤器
    pub fn new(config: CrawlerConfig) -> Self {
        Self { config }
    }

    /// 判断 URL 是否应该被抓取
    ///
    /// 检查顺序：
    /// 1. 深度限制
    /// 2. 域名白名单
    /// 3. 排除模式
    /// 4. URL 协议（只允许 http/https）
    pub fn should_crawl(&self, url: &Url, depth: u32) -> FilterResult {
        // 检查深度
        if depth > self.config.max_depth {
            return FilterResult::Deny("超出最大深度限制");
        }

        // 检查协议
        let scheme = url.scheme();
        if scheme != "http" && scheme != "https" {
            return FilterResult::Deny("不支持的协议");
        }

        // 检查域名白名单
        if !self.config.is_url_allowed(url) {
            return FilterResult::Deny("域名不在白名单中或匹配排除模式");
        }

        FilterResult::Allow
    }

    /// 判断链接是否是内部链接（与给定 URL 同域名）
    pub fn is_internal(base: &Url, target: &Url) -> bool {
        match (base.host_str(), target.host_str()) {
            (Some(base_host), Some(target_host)) => base_host == target_host,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CrawlerConfig;

    fn create_filter() -> UrlFilter {
        let config = CrawlerConfig {
            max_depth: 2,
            allowed_domains: vec!["example.com".to_string()],
            exclude_patterns: vec!["/admin".to_string()],
            ..CrawlerConfig::default_for_test()
        };
        UrlFilter::new(config)
    }

    #[test]
    fn test_allow_valid_url() {
        let filter = create_filter();
        let url = Url::parse("https://example.com/page").unwrap();
        assert!(filter.should_crawl(&url, 1).is_allowed());
    }

    #[test]
    fn test_deny_depth_exceeded() {
        let filter = create_filter();
        let url = Url::parse("https://example.com/page").unwrap();
        let result = filter.should_crawl(&url, 3);
        assert!(!result.is_allowed());
        assert_eq!(result.reason(), Some("超出最大深度限制"));
    }

    #[test]
    fn test_deny_wrong_domain() {
        let filter = create_filter();
        let url = Url::parse("https://other.com/page").unwrap();
        let result = filter.should_crawl(&url, 1);
        assert!(!result.is_allowed());
    }

    #[test]
    fn test_deny_excluded_pattern() {
        let filter = create_filter();
        let url = Url::parse("https://example.com/admin/login").unwrap();
        let result = filter.should_crawl(&url, 1);
        assert!(!result.is_allowed());
    }

    #[test]
    fn test_deny_non_http_protocol() {
        let filter = create_filter();
        let url = Url::parse("ftp://example.com/file").unwrap();
        let result = filter.should_crawl(&url, 1);
        assert!(!result.is_allowed());
        assert_eq!(result.reason(), Some("不支持的协议"));
    }

    #[test]
    fn test_is_internal() {
        let base = Url::parse("https://example.com/page").unwrap();
        let internal = Url::parse("https://example.com/other").unwrap();
        let external = Url::parse("https://other.com/page").unwrap();

        assert!(UrlFilter::is_internal(&base, &internal));
        assert!(!UrlFilter::is_internal(&base, &external));
    }
}
