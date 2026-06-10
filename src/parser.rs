//! HTML 解析模块
//!
//! 负责解析 HTML 内容，提取页面标题、正文和链接。
//! 使用 scraper 库进行 DOM 操作。

use crate::models::{Link, Page};
use scraper::{ElementRef, Html, Selector};
use url::Url;

/// 噪声标签：这些标签及其子元素的内容会被完全忽略
const NOISE_TAGS: &[&str] = &[
    "script", "style", "noscript", "iframe", "svg", "canvas", "nav", "footer", "aside", "header",
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

/// 密度提取候选
#[derive(Debug)]
struct DensityCandidate {
    text: String,
    score: f64,
}

/// 元素统计信息（用于密度计算）
#[derive(Debug)]
struct ElementStats {
    /// 文本字符数
    text_len: usize,
    /// 子孙标签数量
    tag_count: usize,
    /// 合并后的文本
    text: String,
    /// 链接文本字符数
    link_text_len: usize,
}

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
    /// 2. 如果没有语义化标签，使用**文本密度评分**自动识别最大文本块
    /// 3. 最后回退到 `<body>` 提取，跳过已知噪声区域
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

        // 策略 2：基于文本密度自动识别正文区域
        // 适用于没有 article/main 标签的网页
        if let Some(content) = Self::extract_by_density(document) {
            return Some(content);
        }

        // 策略 3：从 body 提取，跳过噪声区域
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

    /// 基于文本密度的正文提取
    ///
    /// 算法：遍历所有块级元素，计算每个元素的"文本密度得分"：
    /// - 文本密度 = 文本字符数 / 标签数量（越高说明单位标签承载的文本越多）
    /// - 链接密度 = 链接文本 / 总文本（越低越不像导航栏/列表）
    /// - 得分 = 文本密度 × (1 - 链接密度) × √文本长度
    ///
    /// 选择得分最高的元素作为正文区域。
    fn extract_by_density(document: &Html) -> Option<String> {
        let body_selector = Selector::parse("body").ok()?;
        let body = document.select(&body_selector).next()?;

        let mut candidates: Vec<DensityCandidate> = Vec::new();
        Self::collect_density_candidates(&body, &mut candidates);

        if candidates.is_empty() {
            return None;
        }

        // 按得分降序排序
        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // 返回得分最高的候选
        candidates.into_iter().next().map(|c| c.text)
    }

    /// 递归收集密度候选
    fn collect_density_candidates(element: &ElementRef, candidates: &mut Vec<DensityCandidate>) {
        // 噪声元素不成为候选，也不递归其子元素
        if Self::is_noise_element(element) {
            return;
        }

        let tag_name = element.value().name();

        // 只考虑容器级标签作为候选
        let is_container = matches!(tag_name, "div" | "section" | "td" | "li" | "blockquote");

        if is_container {
            let stats = Self::calc_element_stats(element);

            // 只保留有足够文本量的候选
            if stats.text_len >= 50 {
                let tag_count = stats.tag_count.max(1) as f64;
                let text_len_f = stats.text_len as f64;
                let link_text_f = stats.link_text_len as f64;

                let text_density = text_len_f / tag_count;
                let link_density = if text_len_f > 0.0 {
                    link_text_f / text_len_f
                } else {
                    0.0
                };

                // 得分公式：高密度 + 低链接密度 + 一定文本量
                let score = text_density * (1.0 - link_density) * text_len_f.sqrt();

                candidates.push(DensityCandidate {
                    text: stats.text,
                    score,
                });
            }
        }

        // 递归到子元素（即使是容器也要递归，让子元素也有机会成为候选）
        for child in element.children() {
            if let Some(child_el) = ElementRef::wrap(child) {
                Self::collect_density_candidates(&child_el, candidates);
            }
        }
    }

    /// 计算元素及其子孙的统计信息
    fn calc_element_stats(element: &ElementRef) -> ElementStats {
        let mut text_parts = Vec::new();
        let mut tag_count = 0;
        let mut link_text_parts = Vec::new();

        Self::calc_stats_recursive(
            element,
            &mut text_parts,
            &mut tag_count,
            &mut link_text_parts,
        );

        let text = Self::join_and_clean(&text_parts);
        let link_text = Self::join_and_clean(&link_text_parts);

        ElementStats {
            text_len: text.chars().count(),
            tag_count,
            text,
            link_text_len: link_text.chars().count(),
        }
    }

    /// 递归计算统计信息
    fn calc_stats_recursive(
        element: &ElementRef,
        text_parts: &mut Vec<String>,
        tag_count: &mut usize,
        link_text_parts: &mut Vec<String>,
    ) {
        if Self::is_noise_element(element) {
            return;
        }

        let tag_name = element.value().name();
        let is_element_tag = tag_name != "body";

        if is_element_tag {
            *tag_count += 1;
        }

        // 判断是否是 <a> 标签（用于计算链接文本）
        let is_link = tag_name == "a";

        for child in element.children() {
            match child.value() {
                scraper::Node::Text(text) => {
                    let t = text.text.trim();
                    if !t.is_empty() {
                        text_parts.push(t.to_string());
                        if is_link {
                            link_text_parts.push(t.to_string());
                        }
                    }
                }
                scraper::Node::Element(_) => {
                    if let Some(child_el) = ElementRef::wrap(child) {
                        Self::calc_stats_recursive(
                            &child_el,
                            text_parts,
                            tag_count,
                            link_text_parts,
                        );
                    }
                }
                _ => {}
            }
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
        assert!(!content.contains("Home")); // nav 内文本被过滤
        assert!(!content.contains("About")); // nav 内文本被过滤
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
        assert!(!content.contains("Home")); // site-navigation
        assert!(!content.contains("Contact")); // site-navigation
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
        assert!(!content.contains("cookies")); // cookie-consent-banner
        assert!(!content.contains("Trending")); // sidebar-widget
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

    // ───────────────────────────────────────────────
    //  文本密度评分测试
    // ───────────────────────────────────────────────

    #[test]
    fn test_density_extracts_largest_text_block() {
        // 没有 article/main 标签，正文和噪声混在一起
        let html = r#"
            <html><body>
                <div>
                    <a href="/">Home</a>
                    <a href="/about">About</a>
                </div>
                <div>
                    <h1>Main Article</h1>
                    <p>This is a long paragraph with lots of text content.</p>
                    <p>It continues with more interesting information here.</p>
                    <p>The third paragraph adds even more substance to the article.</p>
                </div>
                <div>
                    <a href="/link1">Related</a>
                    <a href="/link2">More</a>
                </div>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        // 密度提取应选中最大的文本块（中间 div）
        assert!(content.contains("Main Article"));
        assert!(content.contains("long paragraph"));
        assert!(content.contains("interesting information"));
    }

    #[test]
    fn test_density_prefers_low_link_density() {
        // 两个文本量相近的块，但一个链接密度高（导航），一个低（正文）
        let html = r#"
            <html><body>
                <div>
                    <a href="/a">Link A</a>
                    <a href="/b">Link B</a>
                    <a href="/c">Link C</a>
                    <a href="/d">Link D</a>
                    <a href="/e">Link E</a>
                    <span>Some nav text</span>
                </div>
                <div>
                    <h2>Real Content</h2>
                    <p>This is the actual article body with substantial text.</p>
                    <p>More paragraphs of real content go here.</p>
                </div>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        // 低链接密度的正文块应该胜出
        assert!(content.contains("Real Content"));
        assert!(content.contains("actual article body"));
    }

    #[test]
    fn test_density_with_wrapper_layout() {
        // 真实网页常见结构：wrapper 包裹所有内容
        let html = r#"
            <html><body>
                <div id="wrapper">
                    <div id="nav">
                        <a href="/">Home</a>
                        <a href="/products">Products</a>
                    </div>
                    <div id="content-area">
                        <h1>Product Review</h1>
                        <p>This product is amazing and works very well.</p>
                        <p>We tested it thoroughly and found excellent results.</p>
                        <p>The build quality is top notch and durable.</p>
                    </div>
                    <div id="sidebar">
                        <div class="widget">Categories</div>
                        <div class="widget">Tags</div>
                    </div>
                </div>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        // 噪声 class 被过滤，正文应来自 content-area
        assert!(content.contains("Product Review"));
        assert!(content.contains("product is amazing"));
        assert!(!content.contains("Categories"));
        assert!(!content.contains("Tags"));
    }

    #[test]
    fn test_density_table_layout() {
        // 老式 table 布局网页
        let html = r#"
            <html><body>
                <table>
                    <tr><td>
                        <a href="/">Home</a> | <a href="/contact">Contact</a>
                    </td></tr>
                    <tr><td>
                        <h2>News Article</h2>
                        <p>Breaking news today with detailed reporting.</p>
                        <p>More details about the event unfold here.</p>
                    </td></tr>
                    <tr><td>
                        Copyright 2024
                    </td></tr>
                </table>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        // 应提取到新闻文章区域的文本（td 是容器标签）
        assert!(content.contains("News Article"));
        assert!(content.contains("Breaking news"));
    }

    #[test]
    fn test_density_nested_content() {
        // 嵌套结构：正文在多层 div 内
        let html = r#"
            <html><body>
                <div class="container">
                    <div class="row">
                        <div class="col">
                            <h1>Deep Nested Article</h1>
                            <p>This content is buried deep in nested divs.</p>
                            <p>But it should still be found because it has high text density.</p>
                            <p>Each paragraph adds more text to make this block stand out.</p>
                        </div>
                        <div class="col sidebar">
                            <p>Side info</p>
                        </div>
                    </div>
                </div>
            </body></html>
        "#;

        let base_url = Url::parse("https://example.com").unwrap();
        let page = HtmlParser::parse(&base_url, html, 0, 200, 100);

        let content = page.content.expect("应该有内容");
        assert!(content.contains("Deep Nested Article"));
        assert!(content.contains("buried deep"));
        assert!(!content.contains("Side info"));
    }
}
