//! 配置管理模块
//!
//! 负责加载和验证爬虫配置，包括命令行参数解析和配置验证。

use clap::Parser;
use std::path::PathBuf;

/// 输出格式枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// JSON 格式输出（默认）
    #[default]
    Json,
    /// CSV 格式输出
    Csv,
    /// SQLite 数据库输出
    Sqlite,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(OutputFormat::Json),
            "csv" => Ok(OutputFormat::Csv),
            "sqlite" => Ok(OutputFormat::Sqlite),
            _ => Err(format!("未知的输出格式: {}，可选: json, csv, sqlite", s)),
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Csv => write!(f, "csv"),
            OutputFormat::Sqlite => write!(f, "sqlite"),
        }
    }
}

/// 爬虫配置
///
/// 包含爬虫运行的所有参数，从命令行或配置文件加载。
#[derive(Debug, Clone, Parser)]
#[command(
    name = "rust-crawler",
    about = "一个基于异步运行时的高性能网页爬虫",
    version = "0.1.0"
)]
pub struct CrawlerConfig {
    /// 种子 URL 列表（可指定多个）
    #[arg(short, long = "seed", required = true, help = "种子URL，可多次指定")]
    pub seeds: Vec<String>,

    /// 最大爬取深度
    #[arg(short, long, default_value = "2", help = "最大爬取深度")]
    pub max_depth: u32,

    /// 最大并发请求数
    #[arg(short, long, default_value = "10", help = "最大并发请求数")]
    pub max_concurrency: usize,

    /// 请求超时（秒）
    #[arg(long, default_value = "30", help = "每个请求的超时时间（秒）")]
    pub timeout_secs: u64,

    /// 请求间隔（毫秒）
    #[arg(long, default_value = "100", help = "请求间隔（毫秒），礼貌爬取")]
    pub delay_ms: u64,

    /// 允许爬取的域名白名单（空表示不限制）
    #[arg(long = "allow", help = "允许的域名，可多次指定")]
    pub allowed_domains: Vec<String>,

    /// 排除的 URL 模式（子字符串匹配）
    #[arg(long = "exclude", help = "排除的URL模式，可多次指定")]
    pub exclude_patterns: Vec<String>,

    /// 用户代理字符串
    #[arg(long, default_value = "rust-crawler/0.1.0", help = "User-Agent")]
    pub user_agent: String,

    /// 最大爬取页面数（0 表示不限）
    #[arg(long, default_value = "0", help = "最大爬取页面数，0表示不限")]
    pub max_pages: usize,

    /// 输出格式
    #[arg(short, long, default_value = "json", help = "输出格式: json, csv, sqlite")]
    pub format: OutputFormat,

    /// 输出文件路径
    #[arg(short, long, default_value = "output", help = "输出文件路径（不含扩展名）")]
    pub output: PathBuf,

    /// 是否遵循 robots.txt
    #[arg(long, help = "是否遵循robots.txt")]
    pub respect_robots: bool,

    /// 详细日志输出
    #[arg(short, long, help = "启用详细日志")]
    pub verbose: bool,
}

impl CrawlerConfig {
    /// 创建默认配置（用于测试）
    pub fn default_for_test() -> Self {
        Self {
            seeds: vec!["https://example.com".to_string()],
            max_depth: 1,
            max_concurrency: 5,
            timeout_secs: 10,
            delay_ms: 0,
            allowed_domains: vec![],
            exclude_patterns: vec![],
            user_agent: "rust-crawler/test".to_string(),
            max_pages: 10,
            format: OutputFormat::Json,
            output: PathBuf::from("test_output"),
            respect_robots: false,
            verbose: false,
        }
    }

    /// 验证配置合法性
    pub fn validate(&self) -> Result<(), ConfigError> {
        // 检查种子 URL 是否为空
        if self.seeds.is_empty() {
            return Err(ConfigError::NoSeeds);
        }

        // 验证每个种子 URL 是否合法
        for seed in &self.seeds {
            if let Err(e) = url::Url::parse(seed) {
                return Err(ConfigError::InvalidUrl(seed.clone(), e.to_string()));
            }
        }

        // 检查并发数是否合理
        if self.max_concurrency == 0 {
            return Err(ConfigError::InvalidConcurrency);
        }

        // 检查深度是否合理
        if self.max_depth > 10 {
            return Err(ConfigError::DepthTooLarge(self.max_depth));
        }

        Ok(())
    }

    /// 检查 URL 是否在允许范围内
    pub fn is_url_allowed(&self, url: &url::Url) -> bool {
        // 检查域名白名单
        if !self.allowed_domains.is_empty() {
            let host = url.host_str().unwrap_or("");
            if !self
                .allowed_domains
                .iter()
                .any(|d| host == d.as_str() || host.ends_with(&format!(".{}", d)))
            {
                return false;
            }
        }

        // 检查排除模式
        let url_str = url.as_str();
        for pattern in &self.exclude_patterns {
            if url_str.contains(pattern) {
                return false;
            }
        }

        true
    }

    /// 获取带扩展名的输出路径
    pub fn output_path_with_ext(&self) -> PathBuf {
        let ext = match self.format {
            OutputFormat::Json => "json",
            OutputFormat::Csv => "csv",
            OutputFormat::Sqlite => "db",
        };
        self.output.with_extension(ext)
    }
}

/// 配置错误类型
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("没有指定种子 URL")]
    NoSeeds,

    #[error("无效的 URL '{0}': {1}")]
    InvalidUrl(String, String),

    #[error("并发数必须大于 0")]
    InvalidConcurrency,

    #[error("爬取深度 {0} 过大，建议不超过 10")]
    DepthTooLarge(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_format_from_str() {
        assert_eq!(
            "json".parse::<OutputFormat>().unwrap(),
            OutputFormat::Json
        );
        assert_eq!("csv".parse::<OutputFormat>().unwrap(), OutputFormat::Csv);
        assert_eq!(
            "sqlite".parse::<OutputFormat>().unwrap(),
            OutputFormat::Sqlite
        );
        assert!("unknown".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn test_config_validation() {
        let mut config = CrawlerConfig::default_for_test();
        assert!(config.validate().is_ok());

        // 空种子
        config.seeds.clear();
        assert!(matches!(config.validate(), Err(ConfigError::NoSeeds)));

        // 无效 URL
        config.seeds = vec!["not-a-url".to_string()];
        assert!(
            matches!(config.validate(), Err(ConfigError::InvalidUrl(_, _)))
        );

        // 并发数为 0
        config = CrawlerConfig::default_for_test();
        config.max_concurrency = 0;
        assert!(
            matches!(config.validate(), Err(ConfigError::InvalidConcurrency))
        );
    }

    #[test]
    fn test_is_url_allowed() {
        let config = CrawlerConfig {
            allowed_domains: vec!["example.com".to_string()],
            exclude_patterns: vec!["/admin".to_string()],
            ..CrawlerConfig::default_for_test()
        };

        let allowed = url::Url::parse("https://example.com/page").unwrap();
        let denied_domain = url::Url::parse("https://other.com/page").unwrap();
        let denied_pattern = url::Url::parse("https://example.com/admin").unwrap();

        assert!(config.is_url_allowed(&allowed));
        assert!(!config.is_url_allowed(&denied_domain));
        assert!(!config.is_url_allowed(&denied_pattern));
    }
}
