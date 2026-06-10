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
    /// 3. 包含/排除模式
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

        // 检查包含模式：如果设置了 include_patterns，URL 必须匹配至少一个模式
        let url_str = url.as_str();
        if !self.config.include_patterns.is_empty() {
            let matches_include = self
                .config
                .include_patterns
                .iter()
                .any(|p| url_str.contains(p.as_str()));
            if !matches_include {
                return FilterResult::Deny("URL 不匹配任何包含模式");
            }
        }

        FilterResult::Allow
    }

    /// 判断翻页链接是否应该被抓取（不受 include 和 depth 限制）
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
            include_patterns: vec![],
            page_template: None,
            page_range: None,
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
    fn test_include_pattern_must_match() {
        let config = CrawlerConfig {
            max_depth: 2,
            allowed_domains: vec!["example.com".to_string()],
            include_patterns: vec!["/house/".to_string(), "/zu/".to_string()],
            exclude_patterns: vec![],
            page_template: None,
            page_range: None,
            ..CrawlerConfig::default_for_test()
        };
        let filter = UrlFilter::new(config);

        // 匹配包含模式
        let matching = Url::parse("https://example.com/house/detail.html").unwrap();
        assert!(filter.should_crawl(&matching, 1).is_allowed());

        let matching2 = Url::parse("https://example.com/zu/list.html").unwrap();
        assert!(filter.should_crawl(&matching2, 1).is_allowed());

        // 不匹配包含模式
        let not_matching = Url::parse("https://example.com/news/").unwrap();
        assert!(!filter.should_crawl(&not_matching, 1).is_allowed());
        assert_eq!(filter.should_crawl(&not_matching, 1).reason(), Some("URL 不匹配任何包含模式"));
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
