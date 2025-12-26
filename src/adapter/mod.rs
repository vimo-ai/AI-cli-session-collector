//! Adapter 架构 - 支持多种 CLI 数据源
//!
//! 设计参考 NestJS 版本的 adapter 模式，支持:
//! - Claude Code (JSONL)
//! - Codex CLI (history.jsonl + rollout)
//! - 未来更多 CLI 工具

mod claude;
mod codex;

pub use claude::ClaudeAdapter;
pub use codex::CodexAdapter;

use anyhow::Result;
use crate::domain::{ParseResult, SessionMeta, Source};

/// 会话适配器 trait
pub trait ConversationAdapter: Send + Sync {
    /// 数据来源标识
    fn source(&self) -> Source;

    /// 列出当前来源下的所有会话元数据
    fn list_sessions(&self) -> Result<Vec<SessionMeta>>;

    /// 解析单个会话
    fn parse_session(&self, meta: &SessionMeta) -> Result<Option<ParseResult>>;
}
