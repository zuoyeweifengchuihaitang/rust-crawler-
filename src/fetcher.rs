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

impl FetcherError {
    fn is_transient(&self) -> bool {
        match self {
            FetcherError::HttpError(_) | FetcherError::Timeout(_) => true,
            FetcherError::BadStatus(status) => matches!(status, 429 | 500..=599),
            _ => false,
        }
    }
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
            .redirect(reqwest::redirect::Policy::limited(10))
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
        let response = self.client.get(url).send().await.map_err(|e| {
            if e.is_timeout() {
                FetcherError::Timeout(self.timeout.as_secs())
            } else {
                FetcherError::HttpError(e)
            }
        })?;

        let status = response.status();
        let headers = response.headers().clone();
        let final_url = response.url().to_string();

        // 检查内容类型，只处理 HTML
        let content_type = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // 检查状态码
        if !status.is_success() {
            return Err(FetcherError::BadStatus(status.as_u16()));
        }

        // 检查是否是 HTML
        if let Some(ref ct) = content_type {
            if !ct.contains("text/html") && !ct.contains("application/xhtml+xml") {
                return Err(FetcherError::NotHtml(Some(ct.clone())));
            }
        }

        // 读取响应体原始字节（不依赖 .text() 的 UTF-8 默认假设）
        let body_bytes = response.bytes().await?;

        // 从 Content-Type 中提取 charset，并据此解码
        let charset = content_type
            .as_ref()
            .and_then(|ct| {
                ct.split(';').find_map(|part| {
                    let part = part.trim();
                    let lower = part.to_lowercase();
                    if lower.starts_with("charset=") {
                        Some(part[8..].trim().to_string())
                    } else {
                        None
                    }
                })
            });

        let body = if let Some(ref cs) = charset {
            let cs_lower = cs.to_lowercase();
            match cs_lower.as_str() {
                "utf-8" | "utf8" => String::from_utf8_lossy(&body_bytes).into_owned(),
                "gbk" | "gb2312" | "gb18030" | "gbk2312" => {
                    let (decoded, _had_errors, _replaced) = encoding_rs::GBK.decode(&body_bytes);
                    decoded.into_owned()
                }
                "big5" | "big5-hkscs" => {
                    let (decoded, _had_errors, _replaced) = encoding_rs::BIG5.decode(&body_bytes);
                    decoded.into_owned()
                }
                "shift_jis" | "euc-jp" | "iso-2022-jp" => {
                    let (decoded, _had_errors, _replaced) =
                        encoding_rs::SHIFT_JIS.decode(&body_bytes);
                    decoded.into_owned()
                }
                "euc-kr" | "iso-2022-kr" => {
                    let (decoded, _had_errors, _replaced) =
                        encoding_rs::EUC_KR.decode(&body_bytes);
                    decoded.into_owned()
                }
                "iso-8859-1" | "latin1" => {
                    let (decoded, _had_errors, _replaced) =
                        encoding_rs::WINDOWS_1252.decode(&body_bytes);
                    decoded.into_owned()
                }
                _ => {
                    // 未知编码，先试 UTF-8，失败则回退 GBK
                    if let Ok(s) = String::from_utf8(body_bytes.to_vec()) {
                        s
                    } else {
                        let (decoded, _had_errors, _replaced) =
                            encoding_rs::GBK.decode(&body_bytes);
                        decoded.into_owned()
                    }
                }
            }
        } else {
            // 无 charset 信息：先试 UTF-8，若含大量替换字符则回退 GBK
            let utf8_result = String::from_utf8_lossy(&body_bytes).into_owned();
            // 如果替换字符占比 > 5%，尝试 GBK 解码
            let replacement_count = utf8_result.chars().filter(|&c| c == '\u{FFFD}').count();
            if replacement_count as f64 / utf8_result.len().max(1) as f64 > 0.05 {
                let (decoded, _had_errors, _replaced) = encoding_rs::GBK.decode(&body_bytes);
                decoded.into_owned()
            } else {
                utf8_result
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        // 构建结果
        let result = FetchResult {
            url: final_url,
            status,
            headers,
            body,
            duration_ms,
            content_type: content_type.clone(),
        };

        Ok(result)
    }

    /// 异步获取单个 URL，带自动重试
    pub async fn fetch_with_retries(
        &self,
        url: &str,
        retries: usize,
        retry_delay_ms: u64,
    ) -> Result<FetchResult, FetcherError> {
        let mut last_error = None;

        for attempt in 0..=retries {
            match self.fetch(url).await {
                Ok(result) => return Ok(result),
                Err(err) => {
                    if attempt == retries || !err.is_transient() {
                        return Err(err);
                    }
                    last_error = Some(err);
                    tokio::time::sleep(std::time::Duration::from_millis(retry_delay_ms)).await;
                }
            }
        }

        Err(last_error.expect("fetch_with_retries should always produce an error"))
    }

    /// 获取 User-Agent
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// 获取内部 reqwest 客户端
    pub fn client(&self) -> Client {
        self.client.clone()
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
