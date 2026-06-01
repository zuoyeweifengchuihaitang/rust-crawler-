//! robots.txt 解析和许可管理模块
//!
//! 负责请求 robots.txt 并根据 User-Agent 判断 URL 是否允许抓取。

use reqwest::Client;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::warn;
use url::Url;

/// robots.txt 规则集
#[derive(Debug, Clone, Default)]
pub struct RobotsTxt {
    allow: Vec<String>,
    disallow: Vec<String>,
}

impl RobotsTxt {
    /// 判断给定路径是否允许抓取
    pub fn is_allowed(&self, path: &str) -> bool {
        let allow_match = self
            .allow
            .iter()
            .filter(|pattern| !pattern.is_empty() && path.starts_with(pattern.as_str()))
            .max_by_key(|pattern| pattern.len())
            .map(|pattern| pattern.len());

        let disallow_match = self
            .disallow
            .iter()
            .filter(|pattern| !pattern.is_empty() && path.starts_with(pattern.as_str()))
            .max_by_key(|pattern| pattern.len())
            .map(|pattern| pattern.len());

        match (allow_match, disallow_match) {
            (Some(allow_len), Some(disallow_len)) => allow_len >= disallow_len,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            _ => true,
        }
    }
}

/// robots.txt 管理器
pub struct RobotsManager {
    client: Client,
    user_agent: String,
    cache: RwLock<HashMap<String, RobotsTxt>>,
}

impl RobotsManager {
    /// 创建新的 RobotsManager
    pub fn new(client: Client, user_agent: String) -> Self {
        Self {
            client,
            user_agent,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// 判断 URL 是否允许抓取
    pub async fn is_allowed(&self, url: &Url) -> bool {
        let host = match url.host_str() {
            Some(host) => host,
            None => return true,
        };

        let cache_key = format!("{}://{}", url.scheme(), host);
        if let Some(robots) = self.cache.read().await.get(&cache_key) {
            return robots.is_allowed(url.path());
        }

        let robots_url = match Url::parse(&format!("{}://{}/robots.txt", url.scheme(), host)) {
            Ok(parsed) => parsed,
            Err(err) => {
                warn!("解析 robots.txt URL 失败: {}", err);
                return true;
            }
        };

        let robots = match self.fetch_robots(&robots_url).await {
            Ok(robots) => robots,
            Err(err) => {
                warn!("无法获取 robots.txt {}: {}", robots_url, err);
                RobotsTxt::default()
            }
        };

        self.cache.write().await.insert(cache_key, robots.clone());
        robots.is_allowed(url.path())
    }

    async fn fetch_robots(&self, robots_url: &Url) -> Result<RobotsTxt, reqwest::Error> {
        let response = self.client.get(robots_url.as_str()).send().await?;
        let status = response.status();

        if status.is_client_error() && status.as_u16() == 404 {
            return Ok(RobotsTxt::default());
        }

        let body = response.text().await?;
        Ok(parse_robots_txt(&body, &self.user_agent))
    }
}

fn parse_robots_txt(body: &str, user_agent: &str) -> RobotsTxt {
    #[derive(Default)]
    struct Group {
        user_agents: Vec<String>,
        allow: Vec<String>,
        disallow: Vec<String>,
    }

    let mut groups = Vec::new();
    let mut current = Group::default();
    let mut last_directive_was_user_agent = false;
    let agent = user_agent.to_lowercase();

    for raw_line in body.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.splitn(2, ':');
        let directive = parts.next().unwrap_or("").trim().to_lowercase();
        let value = parts.next().unwrap_or("").trim().to_string();

        match directive.as_str() {
            "user-agent" => {
                if !current.user_agents.is_empty() && !last_directive_was_user_agent {
                    groups.push(current);
                    current = Group::default();
                }
                current.user_agents.push(value.to_lowercase());
                last_directive_was_user_agent = true;
            }
            "allow" => {
                current.allow.push(value);
                last_directive_was_user_agent = false;
            }
            "disallow" => {
                current.disallow.push(value);
                last_directive_was_user_agent = false;
            }
            _ => {
                last_directive_was_user_agent = false;
            }
        }
    }

    if !current.user_agents.is_empty() {
        groups.push(current);
    }

    let default_group = Group::default();
    let selected = groups
        .iter()
        .find(|group| {
            group
                .user_agents
                .iter()
                .any(|ua| ua == "*" || agent.contains(ua))
        })
        .or_else(|| {
            groups
                .iter()
                .find(|group| group.user_agents.iter().any(|ua| ua == "*"))
        })
        .unwrap_or(&default_group);

    RobotsTxt {
        allow: selected.allow.clone(),
        disallow: selected.disallow.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_robots_txt_allow_disallow() {
        let text = "User-agent: *\nDisallow: /admin\nAllow: /admin/public\n";

        let robots = parse_robots_txt(text, "rust-crawler");
        assert!(!robots.is_allowed("/admin"));
        assert!(robots.is_allowed("/admin/public"));
        assert!(robots.is_allowed("/about"));
    }

    #[test]
    fn test_parse_specific_user_agent() {
        let text =
            "User-agent: BadBot\nDisallow: /\n\nUser-agent: rust-crawler\nDisallow: /private\n";

        let robots = parse_robots_txt(text, "rust-crawler/0.1.0");
        assert!(robots.is_allowed("/public"));
        assert!(!robots.is_allowed("/private"));
    }
}
