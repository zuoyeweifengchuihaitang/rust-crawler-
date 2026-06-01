//! HTML 解析模块
//!
//! 负责解析 HTML 内容，提取页面标题、正文和链接。
//! 使用 scraper 库进行 DOM 操作。

use crate::models::{Link, Page};
use scraper::{Html, Selector};
use url::Url;

/// HTML 解析器
///
/// 提供静态方法解析 HTML 内容，无需实例化。
pub struct HtmlParser;

impl HtmlParser {
    /// 解析 HTML 内容，提取页面信息
    ///
    /// # 参数
    /// - `base_url`: 页面所在的基础 URL，用于将相对链接转为绝对链接
    /// - `html`: HTML 文本内容
    /// - `depth`: 当前页面深度
    /// - `status_code`: HTTP 状态码
    /// - `fetch_duration_ms`: 获取耗时
    ///
    /// # 返回
    /// 解析后的 Page 结构
    pub fn parse(
        base_url: &Url,
        html: &str,
        depth: u32,
        status_code: u16,
        fetch_duration_ms: u64,
    ) -> Page {
        let document = Html::parse_document(html);

        let mut page = Page::new(base_url.to_string(), status_code, depth, fetch_duration_ms);

        // 提取标题
        page.title = Self::extract_title(&document);

        // 提取正文
        page.content = Self::extract_content(&document);

        // 提取链接
        page.links = Self::extract_links(base_url, &document);

        page
    }

    /// 提取页面标题
    fn extract_title(document: &Html) -> Option<String> {
        let selector = Selector::parse("title").ok()?;
        document
            .select(&selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
    }

    /// 提取正文内容
    ///
    /// 提取 body 中的文本内容，去除 script 和 style 标签。
    fn extract_content(document: &Html) -> Option<String> {
        // 获取 body 元素
        let body_selector = Selector::parse("body").ok()?;
        let body = document.select(&body_selector).next()?;

        // 提取文本，去除 script/style 内容
        let mut text_parts = Vec::new();
        Self::extract_text_recursive(&body, &mut text_parts);

        let content = text_parts
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        if content.is_empty() {
            None
        } else {
            Some(content)
        }
    }

    /// 递归提取文本节点
    fn extract_text_recursive(element: &scraper::ElementRef, parts: &mut Vec<String>) {
        let tag_name = element.value().name();

        // 跳过 script 和 style 标签
        if tag_name == "script" || tag_name == "style" || tag_name == "noscript" {
            return;
        }

        for child in element.children() {
            match child.value() {
                scraper::Node::Text(text) => {
                    let t = text.text.trim();
                    if !t.is_empty() {
                        parts.push(t.to_string());
                    }
                }
                scraper::Node::Element(_) => {
                    if let Some(child_el) = scraper::ElementRef::wrap(child) {
                        Self::extract_text_recursive(&child_el, parts);
                    }
                }
                _ => {}
            }
        }
    }

    /// 提取所有链接
    ///
    /// 解析所有 `<a href="...">` 标签，将相对 URL 转为绝对 URL。
    fn extract_links(base_url: &Url, document: &Html) -> Vec<Link> {
        let mut links = Vec::new();

        let selector = match Selector::parse("a[href]") {
            Ok(s) => s,
            Err(_) => return links,
        };

        for element in document.select(&selector) {
            let href = element.value().attr("href").unwrap_or("");

            // 跳过空链接和锚点
            if href.is_empty() || href.starts_with('#') {
                continue;
            }

            // 将相对 URL 转为绝对 URL
            let absolute_url = match base_url.join(href) {
                Ok(url) => url,
                Err(_) => continue,
            };

            // 只保留 http 和 https
            if absolute_url.scheme() != "http" && absolute_url.scheme() != "https" {
                continue;
            }

            // 获取链接文本
            let text = element
                .text()
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");

            // 判断是否是内部链接
            let is_internal = Self::is_internal_link(base_url, &absolute_url);

            links.push(Link::new(text, absolute_url.to_string(), is_internal));
        }

        links
    }

    /// 判断链接是否是内部链接
    fn is_internal_link(base: &Url, target: &Url) -> bool {
        match (base.host_str(), target.host_str()) {
            (Some(base_host), Some(target_host)) => base_host == target_host,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head><title>Test Page</title></head>
            <body>
                <h1>Hello World</h1>
                <a href="/page1">Page 1</a>
                <a href="https://other.com">External</a>
            </body>
            </html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        assert_eq!(page.url, "https://example.com/");
        assert_eq!(page.title, Some("Test Page".to_string()));
        assert_eq!(page.status_code, 200);
        assert_eq!(page.links.len(), 2);

        // 检查内部链接
        let internal = page.links.iter().find(|l| l.is_internal);
        assert!(internal.is_some());
        assert_eq!(internal.unwrap().url, "https://example.com/page1");

        // 检查外部链接
        let external = page.links.iter().find(|l| !l.is_internal);
        assert!(external.is_some());
        assert_eq!(external.unwrap().url, "https://other.com/");
    }

    #[test]
    fn test_extract_content() {
        let html = r#"
            <html><body>
                <p>First paragraph</p>
                <script>alert('xss')</script>
                <p>Second paragraph</p>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        assert!(content.contains("First paragraph"));
        assert!(content.contains("Second paragraph"));
        assert!(!content.contains("alert")); // script 内容应该被过滤
    }

    #[test]
    fn test_skip_anchor_links() {
        let html = r##"
            <html><body>
                <a href="#section1">Section</a>
                <a href="/page">Page</a>
            </body></html>
        "##;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        assert_eq!(page.links.len(), 1); // 只有 /page，#section1 被跳过
        assert_eq!(page.links[0].url, "https://example.com/page");
    }
}
