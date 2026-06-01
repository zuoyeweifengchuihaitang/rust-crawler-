//! URL 去重模块
//!
//! 负责确保同一 URL 不会被重复抓取。
//! 提供两种实现：基于 HashSet 的精确去重，以及基于 BloomFilter 的近似去重（可选）。

use std::collections::HashSet;
use tokio::sync::RwLock;

/// URL 去重器 trait
///
/// 抽象了去重操作，允许不同的实现策略。
#[async_trait::async_trait]
pub trait UrlDeduper: Send + Sync {
    /// 尝试将 URL 加入已抓取集合
    ///
    /// 返回 true 表示是新 URL（之前未见过），false 表示已存在。
    async fn try_add(&self, url: &str) -> bool;

    /// 获取已记录的 URL 数量
    async fn len(&self) -> usize;

    /// 判断是否为空
    async fn is_empty(&self) -> bool;
}

/// 基于内存 HashSet 的精确去重器
///
/// 使用 `RwLock<HashSet>` 实现线程安全的并发访问。
/// 适合单机场景，内存占用随 URL 数量线性增长。
pub struct MemoryDeduper {
    seen: RwLock<HashSet<String>>,
}

impl MemoryDeduper {
    /// 创建新的内存去重器
    pub fn new() -> Self {
        Self {
            seen: RwLock::new(HashSet::new()),
        }
    }

    /// 创建并预分配容量的去重器
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            seen: RwLock::new(HashSet::with_capacity(capacity)),
        }
    }
}

impl Default for MemoryDeduper {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl UrlDeduper for MemoryDeduper {
    async fn try_add(&self, url: &str) -> bool {
        let mut seen = self.seen.write().await;
        seen.insert(url.to_string())
    }

    async fn len(&self) -> usize {
        self.seen.read().await.len()
    }

    async fn is_empty(&self) -> bool {
        self.seen.read().await.is_empty()
    }
}

/// 创建默认的去重器
///
/// 根据预估的 URL 数量选择合适的实现。
pub fn create_deduper() -> Box<dyn UrlDeduper> {
    Box::new(MemoryDeduper::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_deduper_basic() {
        let deduper = MemoryDeduper::new();

        // 第一次添加应该成功
        assert!(deduper.try_add("https://example.com").await);
        assert_eq!(deduper.len().await, 1);

        // 重复添加应该失败
        assert!(!deduper.try_add("https://example.com").await);
        assert_eq!(deduper.len().await, 1);

        // 不同的 URL 应该成功
        assert!(deduper.try_add("https://other.com").await);
        assert_eq!(deduper.len().await, 2);
    }

    #[tokio::test]
    async fn test_deduper_concurrent() {
        let deduper = std::sync::Arc::new(MemoryDeduper::new());
        let mut handles = vec![];

        // 并发添加相同的 URL
        for i in 0..10 {
            let d = deduper.clone();
            handles.push(tokio::spawn(async move {
                d.try_add(&format!("https://example.com/{}", i % 3))
                    .await
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await.unwrap());
        }

        // 应该有 3 个成功（3 个不同的 URL），7 个失败
        let success_count = results.iter().filter(|&&r| r).count();
        assert_eq!(success_count, 3);
        assert_eq!(deduper.len().await, 3);
    }
}
