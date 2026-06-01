# 🕷️ rust-crawler

一个基于异步运行时的高性能网页爬虫，使用 Rust 编写。

本项目是 **Rust 程序设计课程期末作业**，展示了 Rust 的核心特性：所有权与借用系统、泛型与 trait 抽象、异步编程、错误处理以及模块化设计。

## ✨ 功能特性

- **异步并发抓取**：基于 `tokio` 运行时，使用信号量控制并发数
- **智能 URL 去重**：避免重复抓取同一页面
- **深度与域名控制**：支持爬取深度限制和域名白名单
- **多种输出格式**：支持 JSON、CSV、SQLite 三种格式
- **优雅的错误处理**：全链路 `Result<T, E>`，自定义错误类型
- **模块化设计**：清晰的模块划分，trait 抽象存储后端
- **进度显示**：终端实时显示爬取进度
- **礼貌爬取**：支持请求间隔，遵循 robots.txt（可选），并可在请求失败时自动重试

## 🚀 快速开始

### 安装依赖

```bash
# 克隆项目
cd rust-crawler

# 编译
cargo build --release
```

### 基础用法

```bash
# 从单个种子开始，深度2，JSON输出
cargo run -- --seed https://example.com --max-depth 2 --output results

# 多种子 + 域名限制 + CSV输出
cargo run -- \
  --seed https://docs.rs \
  --seed https://crates.io \
  --allow docs.rs \
  --allow crates.io \
  --max-depth 3 \
  --format csv \
  --output results

# SQLite + 并发控制 + 详细日志
RUST_LOG=debug cargo run -- \
  --seed https://rust-lang.org \
  --max-depth 2 \
  --max-concurrency 10 \
  --format sqlite \
  --output crawl.db \
  --verbose
```

### CLI 参数

```
Usage: rust-crawler [OPTIONS] --seed <SEED>...

Options:
  -s, --seed <SEED>          种子URL，可多次指定
  -d, --max-depth <DEPTH>    最大爬取深度 [默认: 2]
  -c, --max-concurrency <N>  最大并发请求数 [默认: 10]
      --timeout-secs <SEC>   请求超时时间 [默认: 30]
      --retry-count <N>      请求失败重试次数 [默认: 2]
      --retry-delay-ms <MS>  重试间隔 [默认: 500]
      --delay-ms <MS>        请求间隔 [默认: 100]
      --allow <DOMAIN>       允许的域名
      --exclude <PATTERN>    排除的URL模式
      --user-agent <AGENT>   User-Agent [默认: rust-crawler/0.1.0]
      --max-pages <N>        最大爬取页面数 [默认: 0=不限]
  -f, --format <FORMAT>      输出格式: json, csv, sqlite [默认: json]
  -o, --output <PATH>        输出文件路径 [默认: output]
      --respect-robots       遵循robots.txt
  -v, --verbose              启用详细日志
  -h, --help                 打印帮助信息
```

## 🏗️ 项目结构

```
rust-crawler/
├── Cargo.toml           # 项目配置
├── src/
│   ├── main.rs          # CLI入口
│   ├── lib.rs           # 模块聚合
│   ├── models.rs        # 核心数据结构 (Page, Link, CrawlStats)
│   ├── config.rs        # 配置管理 (CrawlerConfig, OutputFormat)
│   ├── crawler.rs       # 核心调度器 (Crawler::run())
│   ├── fetcher.rs       # HTTP获取 (Fetcher)
│   ├── parser.rs        # HTML解析 (HtmlParser)
│   ├── filter.rs        # URL过滤 (UrlFilter)
│   ├── deduper.rs       # URL去重 (MemoryDeduper)
│   └── storage.rs       # 数据持久化 (JsonStorage, CsvStorage, SqliteStorage)
├── tests/
│   └── integration_tests.rs  # 集成测试
└── DESIGN.md            # 设计文档
```

## 🧪 测试

```bash
# 运行所有测试
cargo test

# 运行单元测试
cargo test --lib

# 运行集成测试
cargo test --test integration_tests

# 生成测试覆盖率报告
cargo tarpaulin --out Html
```

## ✅ GitHub Actions CI

项目已配置 GitHub Actions CI，自动执行：

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`

CI 配置文件位于 `.github/workflows/ci.yml`。

## 📊 技术栈

| 用途 | 库 |
|------|------|
| 异步运行时 | `tokio` |
| HTTP 客户端 | `reqwest` |
| HTML 解析 | `scraper` |
| CLI 参数 | `clap` |
| 序列化 | `serde` |
| CSV 导出 | `csv` |
| SQLite | `rusqlite` |
| 进度条 | `indicatif` |
| 日志 | `tracing` |
| 错误处理 | `thiserror` |
| 异步 trait | `async-trait` |

## 🔧 工程规范

```bash
# 代码格式化
cargo fmt

# 代码检查
cargo clippy -- -D warnings

# 构建发布版本
cargo build --release
```

## 📝 许可证

MIT License
