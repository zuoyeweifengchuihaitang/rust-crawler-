//! HTML 解析模块
//!
//! 负责解析 HTML 内容，提取页面标题、正文和链接。
//! 使用 scraper 库进行 DOM 操作。

use crate::models::{Link, Page};
use scraper::{ElementRef, Html, Selector};
use url::Url;

/// 噪声标签：这些标签及其子元素的内容会被完全忽略
const NOISE_TAGS: &[&str] = &[
    "script",
    "style",
    "noscript",
    "iframe",
    "svg",
    "canvas",
    "nav",
    "footer",
    "aside",
    "header",
    "menu",
];

/// 噪声属性模式（用于 class / id 属性，大小写不敏感）。
/// 如果元素的 class 或 id 包含以下子串，则视为噪声区域。
const NOISE_PATTERNS: &[&str] = &[
    "nav",
    "navigation",
    "navbar",
    "menu",
    "sidebar",
    "footer",
    "header",
    "banner",
    "ad",
    "ads",
    "advertisement",
    "advert",
    "cookie",
    "consent",
    "social",
    "share",
    "comment",
    "related",
    "recommend",
    "widget",
    "popup",
    "modal",
    "newsletter",
    "subscribe",
];

/// 主要内容标签：优先从这些标签提取正文
const CONTENT_TAGS: &[&str] = &["article", "main"];

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
    /// 策略：
    /// 1. 优先从 `<article>` 或 `<main>` 标签提取（语义化内容区域）
    /// 2. 如果没有，从 `<body>` 提取，但跳过噪声区域（导航栏、页脚、广告等）
    fn extract_content(document: &Html) -> Option<String> {
        // 策略 1：尝试从语义化内容标签提取
        for tag in CONTENT_TAGS {
            let selector = Selector::parse(tag).ok()?;
            if let Some(el) = document.select(&selector).next() {
                let mut text_parts = Vec::new();
                Self::extract_text_recursive(&el, &mut text_parts);
                let content = Self::join_and_clean(&text_parts);
                if !content.is_empty() {
                    return Some(content);
                }
            }
        }

        // 策略 2：从 body 提取，跳过噪声区域
        let body_selector = Selector::parse("body").ok()?;
        let body = document.select(&body_selector).next()?;

        let mut text_parts = Vec::new();
        Self::extract_text_recursive(&body, &mut text_parts);
        let content = Self::join_and_clean(&text_parts);

        if content.is_empty() {
            None
        } else {
            Some(content)
        }
    }

    /// 递归提取文本节点，自动跳过噪声子树
    fn extract_text_recursive(element: &ElementRef, parts: &mut Vec<String>) {
        // 如果当前元素本身就是噪声，跳过整个子树
        if Self::is_noise_element(element) {
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
                    if let Some(child_el) = ElementRef::wrap(child) {
                        Self::extract_text_recursive(&child_el, parts);
                    }
                }
                _ => {}
            }
        }
    }

    /// 判断元素是否是噪声区域
    ///
    /// 基于两个维度判断：
    /// 1. 标签名是否在噪声标签列表中
    /// 2. class 或 id 属性是否匹配噪声模式
    fn is_noise_element(element: &ElementRef) -> bool {
        let tag_name = element.value().name();

        // 维度 1：噪声标签名
        if NOISE_TAGS.contains(&tag_name) {
            return true;
        }

        // 维度 2：class / id 属性匹配
        let attrs = element.value().attrs();
        for (name, value) in attrs {
            let name_lower = name.to_ascii_lowercase();
            if name_lower != "class" && name_lower != "id" {
                continue;
            }
            let value_lower = value.to_ascii_lowercase();
            for pattern in NOISE_PATTERNS {
                if value_lower.contains(pattern) {
                    return true;
                }
            }
        }

        false
    }

    /// 将文本片段合并并清理空白
    fn join_and_clean(parts: &[String]) -> String {
        parts
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
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

    // ───────────────────────────────────────────────
    //  正文噪声过滤测试
    // ───────────────────────────────────────────────

    #[test]
    fn test_skip_nav_tag() {
        let html = r#"
            <html><body>
                <nav><a href="/">Home</a> <a href="/about">About</a></nav>
                <p>This is the real content.</p>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        assert!(content.contains("real content"));
        assert!(!content.contains("Home"));   // nav 内文本被过滤
        assert!(!content.contains("About"));  // nav 内文本被过滤
    }

    #[test]
    fn test_skip_footer_tag() {
        let html = r#"
            <html><body>
                <p>Article body here.</p>
                <footer>
                    <p>Copyright 2024 Example Inc.</p>
                    <a href="/privacy">Privacy Policy</a>
                </footer>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        assert!(content.contains("Article body"));
        assert!(!content.contains("Copyright"));
        assert!(!content.contains("Privacy"));
    }

    #[test]
    fn test_skip_aside_tag() {
        let html = r#"
            <html><body>
                <p>Main text.</p>
                <aside>
                    <p>Related articles: Foo, Bar, Baz</p>
                </aside>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        assert!(content.contains("Main text"));
        assert!(!content.contains("Related articles"));
    }

    #[test]
    fn test_skip_noise_by_class() {
        let html = r#"
            <html><body>
                <div class="site-navigation">
                    <a href="/home">Home</a>
                    <a href="/contact">Contact</a>
                </div>
                <div class="article-content">
                    <p>Real article text goes here.</p>
                </div>
                <div class="advertisement-banner">
                    <p>Buy our products now!</p>
                </div>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        assert!(content.contains("Real article text"));
        assert!(!content.contains("Home"));           // site-navigation
        assert!(!content.contains("Contact"));        // site-navigation
        assert!(!content.contains("Buy our products")); // advertisement-banner
    }

    #[test]
    fn test_skip_noise_by_id() {
        let html = r#"
            <html><body>
                <div id="cookie-consent-banner">
                    <p>We use cookies to improve your experience.</p>
                </div>
                <div id="main-content">
                    <p>This is the actual content.</p>
                </div>
                <div id="sidebar-widget">
                    <p>Trending topics</p>
                </div>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        assert!(content.contains("actual content"));
        assert!(!content.contains("cookies"));    // cookie-consent-banner
        assert!(!content.contains("Trending"));   // sidebar-widget
    }

    #[test]
    fn test_priority_article_tag() {
        let html = r#"
            <html><body>
                <header>
                    <h1>Site Title</h1>
                </header>
                <article>
                    <h2>Article Heading</h2>
                    <p>Article paragraph one.</p>
                    <p>Article paragraph two.</p>
                </article>
                <footer>
                    <p>Footer info</p>
                </footer>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        // article 优先提取，header/footer 应该被排除
        assert!(content.contains("Article Heading"));
        assert!(content.contains("paragraph one"));
        assert!(!content.contains("Site Title")); // header 被跳过
        assert!(!content.contains("Footer info")); // footer 被跳过
    }

    #[test]
    fn test_priority_main_tag() {
        let html = r#"
            <html><body>
                <nav>Nav content</nav>
                <main>
                    <h1>Main Content Title</h1>
                    <p>Important information here.</p>
                </main>
                <aside>Side content</aside>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        assert!(content.contains("Main Content Title"));
        assert!(content.contains("Important information"));
        assert!(!content.contains("Nav content"));
        assert!(!content.contains("Side content"));
    }

    #[test]
    fn test_skip_nested_noise() {
        let html = r#"
            <html><body>
                <div class="content">
                    <p>Real content before.</p>
                    <div class="share-buttons">
                        <span>Share on Twitter</span>
                        <span>Share on Facebook</span>
                    </div>
                    <p>Real content after.</p>
                </div>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        assert!(content.contains("Real content before"));
        assert!(content.contains("Real content after"));
        assert!(!content.contains("Share on Twitter"));
        assert!(!content.contains("Share on Facebook"));
    }

    #[test]
    fn test_skip_social_and_comments() {
        let html = r#"
            <html><body>
                <article>
                    <p>Interesting article text.</p>
                </article>
                <div class="social-media-links">
                    <p>Follow us on Twitter</p>
                </div>
                <div id="comment-section">
                    <p>User1: Great post!</p>
                    <p>User2: Thanks for sharing.</p>
                </div>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        assert!(content.contains("Interesting article"));
        assert!(!content.contains("Follow us"));
        assert!(!content.contains("Great post"));
        assert!(!content.contains("Thanks for sharing"));
    }

    #[test]
    fn test_combined_real_world_layout() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head><title>Real World Page</title></head>
            <body>
                <header class="site-header">
                    <nav class="main-nav">
                        <a href="/">Home</a>
                        <a href="/about">About</a>
                    </nav>
                </header>
                <main>
                    <article class="post">
                        <h1>How to Write Rust</h1>
                        <p>Rust is a systems programming language.</p>
                        <p>It guarantees memory safety.</p>
                    </article>
                </main>
                <aside class="sidebar">
                    <div class="widget">Popular Posts</div>
                </aside>
                <footer class="site-footer">
                    <p>All rights reserved.</p>
                </footer>
                <div id="newsletter-popup">
                    <p>Subscribe to our newsletter!</p>
                </div>
            </body>
            </html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        // main > article 优先提取
        assert!(content.contains("How to Write Rust"));
        assert!(content.contains("systems programming"));
        assert!(content.contains("memory safety"));

        // 噪声应该被过滤
        assert!(!content.contains("Home"));
        assert!(!content.contains("About"));
        assert!(!content.contains("Popular Posts"));
        assert!(!content.contains("All rights reserved"));
        assert!(!content.contains("Subscribe"));
    }

    #[test]
    fn test_no_article_fallback_to_body() {
        let html = r#"
            <html><body>
                <div class="content">
                    <p>No article or main tag here.</p>
                    <p>Just plain divs with content.</p>
                </div>
                <div class="sidebar">Noise</div>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        assert!(content.contains("No article or main tag"));
        assert!(content.contains("plain divs"));
        assert!(!content.contains("Noise")); // sidebar 被过滤
    }

    #[test]
    fn test_empty_content_returns_none() {
        let html = r#"
            <html><body>
                <nav></nav>
                <footer></footer>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        assert!(page.content.is_none());
    }
}
