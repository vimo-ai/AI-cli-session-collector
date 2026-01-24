//! ai-cli-session-collector - AI CLI 会话收集器
//!
//! 支持收集多种 AI CLI 工具的会话数据：
//! - Claude Code (JSONL)
//! - Codex CLI (history.jsonl + rollout)
//! - OpenCode (JSON)
//!
//! ## 架构
//!
//! 每个适配器通过 `AdapterMeta` 完整描述自己：
//! - 数据来源、默认路径、环境变量、文件扩展名
//!
//! 使用 `all_adapters()` 获取所有适配器实例，
//! 使用 `all_watch_configs()` 获取监听配置。
//!
//! ## 添加新适配器
//!
//! 1. 创建适配器文件，定义 META 常量
//! 2. 实现 `ConversationAdapter` trait
//! 3. 在 `all_adapters()` 注册
//! 4. 完成！

pub mod adapter;
pub mod domain;

// Re-export 适配器相关类型
pub use adapter::{
    // 核心类型
    AdapterMeta,
    // 具体适配器
    ClaudeAdapter,
    CodexAdapter,
    ConversationAdapter,
    OpenCodeAdapter,
    WatchConfig,
    // 工厂函数
    adapter_for_path,
    all_adapters,
    all_extensions,
    all_watch_configs,
};

// Re-export 领域类型
pub use domain::{
    IndexableMessage, IndexableSession, MessageType, ParseResult, ParsedContent, ParsedMessage,
    SessionMeta, Source,
};
