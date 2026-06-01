//! HTTP 获取模块
//!
//! 负责发送异步 HTTP 请求并获取页面内容。
//! 封装了 reqwest 客户端，提供统一的错误处理和超时控制。

use reqwest::{Client, StatusCode};
use std::time::{Duration, Instant};

/// HTTP 获取器
///
/// 封装了 reqwest 客户端，提供异步页面获取功能。
pub struct Fetcher {
    client: Client,
    user_agent: String,
    timeout: Duration,
}

/// 获取结果
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// 请求的 URL
    pub url: String,
    /// HTTP 状态码
    pub status: StatusCode,
    /// 响应头
    #[allow(dead_code)]
    pub headers: reqwest::header::HeaderMap,
    /// 响应体（文本）
    pub body: String,
    /// 获取耗时（毫秒）
    pub duration_ms: u64,
    /// 内容类型
    #[allow(dead_code)]
    pub content_type: Option<String>,
}

/// 获取错误类型
#[derive(Debug, thiserror::Error)]
pub enum FetcherError {
    #[error("HTTP 请求失败: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("URL 解析错误: {0}")]
    UrlError(#[from] url::ParseError),

    #[error("请求超时（超过 {0} 秒）")]
    Timeout(u64),

    #[error("无效的状态码: {0}")]
    BadStatus(u16),

    #[error("非 HTML 内容类型: {0:?}")]
    NotHtml(Option<String>),
}

impl Fetcher {
    /// 创建新的 HTTP 获取器
    ///
    /// # 参数
    /// - `user_agent`: User-Agent 字符串
    /// - `timeout_secs`: 请求超时时间（秒）
    pub fn new(user_agent: &str, timeout_secs: u64) -> Result<Self, FetcherError> {
        let timeout = Duration::from_secs(timeout_secs);
        let client = Client::builder()
            .timeout(timeout)
            .user_agent(user_agent)
            .pool_max_idle_per_host(10)
            .build()?;

        Ok(Self {
            client,
            user_agent: user_agent.to_string(),
            timeout,
        })
    }

    /// 异步获取单个 URL
    ///
    /// 返回页面内容或错误。
    pub async fn fetch(&self, url: &str) -> Result<FetchResult, FetcherError> {
        let start = Instant::now();

        // 发送 GET 请求
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    FetcherError::Timeout(self.timeout.as_secs())
                } else {
                    FetcherError::HttpError(e)
                }
            })?;

        let status = response.status();
        let headers = response.headers().clone();

        // 检查内容类型，只处理 HTML
        let content_type = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // 读取响应体
        let body = response.text().await?;

        let duration_ms = start.elapsed().as_millis() as u64;

        // 构建结果
        let result = FetchResult {
            url: url.to_string(),
            status,
            headers,
            body,
            duration_ms,
            content_type: content_type.clone(),
        };

        // 检查状态码
        if !status.is_success() {
            return Err(FetcherError::BadStatus(status.as_u16()));
        }

        // 检查是否是 HTML
        if let Some(ref ct) = content_type {
            if !ct.contains("text/html") {
                return Err(FetcherError::NotHtml(Some(ct.clone())));
            }
        }

        Ok(result)
    }

    /// 获取 User-Agent
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// 获取超时时间
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 注意：这些测试需要网络连接，实际运行可能需要 mock
    // 这里只是测试 Fetcher 的创建

    #[test]
    fn test_fetcher_creation() {
        let fetcher = Fetcher::new("test-agent", 10);
        assert!(fetcher.is_ok());

        let fetcher = fetcher.unwrap();
        assert_eq!(fetcher.user_agent(), "test-agent");
        assert_eq!(fetcher.timeout().as_secs(), 10);
    }
}
