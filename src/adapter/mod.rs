//! 适配器架构 - 多数据源自注册模式
//!
//! 支持的数据源：
//! - Claude Code (JSONL)
//! - Codex CLI (history.jsonl + rollout)
//! - OpenCode (JSON)
//! - Gemini CLI (session JSON)
//!
//! ## 架构设计
//!
//! 每个适配器通过 `AdapterMeta` 完整描述自己的配置：
//! - 数据来源标识
//! - 默认数据路径
//! - 环境变量覆盖
//! - 监听的文件扩展名
//!
//! ## 添加新适配器
//!
//! 1. 创建适配器文件 (如 `newtool.rs`)
//! 2. 定义 `NEWTOOL_META: AdapterMeta` 常量
//! 3. 实现 `ConversationAdapter` trait
//! 4. 在 `mod.rs` 中导出并在 `all_adapters()` 注册
//! 5. 完成！无需修改 config/registry/watcher

mod claude;
mod codex;
mod gemini;
mod opencode;

pub use claude::ClaudeAdapter;
pub use codex::CodexAdapter;
pub use gemini::GeminiAdapter;
pub use opencode::OpenCodeAdapter;

use crate::domain::{ParseResult, SessionMeta, Source};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ============================================================================
// 适配器元信息
// ============================================================================

/// 适配器静态元信息
///
/// 每个适配器定义一个 const META，包含所有配置
#[derive(Debug, Clone)]
pub struct AdapterMeta {
    /// 数据来源标识
    pub source: Source,
    /// 适配器名称（用于日志）
    pub name: &'static str,
    /// 默认数据路径（相对于 HOME）
    pub default_path: &'static str,
    /// 环境变量名（用于覆盖默认路径）
    pub env_var: &'static str,
    /// 监听的文件扩展名
    pub extensions: &'static [&'static str],
    /// 是否递归监听子目录
    pub recursive: bool,
}

// ============================================================================
// 适配器 Trait
// ============================================================================

/// 会话适配器 trait
///
/// 所有数据源适配器必须实现此 trait
pub trait ConversationAdapter: Send + Sync {
    /// 获取适配器元信息
    fn meta(&self) -> &'static AdapterMeta;

    /// 获取实际数据路径（可能被环境变量覆盖）
    fn data_path(&self) -> &Path;

    /// 列出当前来源下的所有会话元数据
    fn list_sessions(&self) -> Result<Vec<SessionMeta>>;

    /// 解析单个会话
    fn parse_session(&self, meta: &SessionMeta) -> Result<Option<ParseResult>>;

    // ========== 便捷方法（有默认实现）==========

    /// 数据来源标识
    fn source(&self) -> Source {
        self.meta().source
    }

    /// 支持的文件扩展名
    fn extensions(&self) -> &[&str] {
        self.meta().extensions
    }

    /// 检查文件是否属于此适配器处理范围
    fn should_handle(&self, file_path: &Path) -> bool {
        // 1. 检查路径前缀
        if !file_path.starts_with(self.data_path()) {
            return false;
        }

        // 2. 检查扩展名
        let ext = match file_path.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => return false,
        };

        self.meta().extensions.contains(&ext)
    }
}

// ============================================================================
// 适配器注册表
// ============================================================================

/// 创建所有适配器（使用默认/环境变量配置的路径）
///
/// 添加新适配器只需在这里添加一行
pub fn all_adapters() -> Vec<Arc<dyn ConversationAdapter>> {
    vec![
        Arc::new(ClaudeAdapter::new()),
        Arc::new(CodexAdapter::new()),
        Arc::new(OpenCodeAdapter::new()),
        Arc::new(GeminiAdapter::new()),
    ]
}

/// 监听配置（给 watcher 使用）
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// 监听路径
    pub path: PathBuf,
    /// 文件扩展名
    pub extensions: Vec<&'static str>,
    /// 是否递归
    pub recursive: bool,
    /// 数据来源（用于日志）
    pub source: Source,
    /// 适配器名称（用于日志）
    pub name: &'static str,
}

/// 获取所有监听配置
///
/// 只返回路径存在的适配器配置
pub fn all_watch_configs() -> Vec<WatchConfig> {
    all_adapters()
        .into_iter()
        .filter_map(|adapter| {
            let path = adapter.data_path();
            if path.exists() {
                Some(WatchConfig {
                    path: path.to_path_buf(),
                    extensions: adapter.meta().extensions.to_vec(),
                    recursive: adapter.meta().recursive,
                    source: adapter.meta().source,
                    name: adapter.meta().name,
                })
            } else {
                tracing::debug!(
                    "跳过不存在的路径: {} ({:?})",
                    adapter.meta().name,
                    path
                );
                None
            }
        })
        .collect()
}

/// 根据文件路径找到对应的适配器
pub fn adapter_for_path(file_path: &Path) -> Option<Arc<dyn ConversationAdapter>> {
    all_adapters()
        .into_iter()
        .find(|adapter| adapter.should_handle(file_path))
}

/// 获取所有支持的文件扩展名（去重）
pub fn all_extensions() -> Vec<&'static str> {
    let mut exts: Vec<&'static str> = all_adapters()
        .iter()
        .flat_map(|a| a.meta().extensions.iter().copied())
        .collect();
    exts.sort();
    exts.dedup();
    exts
}
