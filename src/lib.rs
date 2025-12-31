//! ai-cli-session-collector - AI CLI 会话收集器
//!
//! 支持收集多种 AI CLI 工具的会话数据：
//! - Claude Code (JSONL)
//! - Codex CLI (history.jsonl + rollout)
//! - 未来更多...
//!
//! 这是一个纯解析库，不包含 IO 依赖（如数据库、HTTP 等）。

pub mod adapter;
pub mod domain;

// Re-export 常用类型
pub use adapter::{ClaudeAdapter, CodexAdapter, ConversationAdapter};
pub use domain::{IndexableMessage, IndexableSession, MessageType, ParsedContent, ParseResult, ParsedMessage, SessionMeta, Source};
