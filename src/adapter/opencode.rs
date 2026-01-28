//! OpenCode 数据适配器
//!
//! 解析 ~/.local/share/opencode/storage/
//! - session/{projectID}/ses_{sessionID}.json
//! - message/{messageID}/msg_{messageID}.json
//! - part/{messageID}/prt_{partID}.json

use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use super::{AdapterMeta, ConversationAdapter};
use crate::domain::{MessageType, ParseResult, ParsedContent, ParsedMessage, SessionMeta, Source};

// ============================================================================
// 适配器元信息（静态配置）
// ============================================================================

/// OpenCode 适配器元信息
pub static OPENCODE_META: AdapterMeta = AdapterMeta {
    source: Source::OpenCode,
    name: "OpenCode",
    default_path: ".local/share/opencode",
    env_var: "OPENCODE_PATH",
    extensions: &["json"],
    recursive: true,
};

/// OpenCode Session JSON 结构
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SessionJson {
    id: String,
    slug: String,
    version: String,
    #[serde(rename = "projectID")]
    project_id: String,
    directory: String,
    title: Option<String>,
    time: SessionTime,
    summary: Option<SessionSummary>,
}

/// OpenCode Session 时间
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SessionTime {
    created: i64,
    updated: i64,
}

/// OpenCode Session 摘要
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SessionSummary {
    additions: u64,
    deletions: u64,
    files: u64,
}

/// OpenCode Message JSON 结构
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MessageJson {
    id: String,
    #[serde(rename = "sessionID")]
    session_id: String,
    role: String,
    time: MessageTime,
    #[serde(rename = "parentID")]
    parent_id: Option<String>,
    #[serde(rename = "modelID")]
    model_id: Option<String>,
    #[serde(rename = "providerID")]
    provider_id: Option<String>,
    mode: Option<String>,
    agent: Option<String>,
    path: Option<MessagePath>,
    cost: Option<f64>,
    tokens: Option<TokenInfo>,
    finish: Option<String>,
}

/// OpenCode Message 时间
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MessageTime {
    created: i64,
    completed: Option<i64>,
}

/// OpenCode Message 路径信息
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MessagePath {
    cwd: Option<String>,
    root: Option<String>,
}

/// OpenCode Token 信息
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TokenInfo {
    input: u64,
    output: u64,
    reasoning: u64,
    #[serde(rename = "cache")]
    cache_info: Option<CacheInfo>,
}

/// OpenCode Cache 信息
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CacheInfo {
    read: u64,
    write: u64,
}

/// OpenCode Part JSON 结构
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PartJson {
    id: String,
    #[serde(rename = "sessionID")]
    session_id: String,
    #[serde(rename = "messageID")]
    message_id: String,
    #[serde(rename = "type")]
    type_: String,
    text: Option<String>,
    time: Option<PartTime>,
    #[serde(rename = "callID")]
    call_id: Option<String>,
    tool: Option<String>,
    state: Option<ToolState>,
    hash: Option<String>,
    snapshot: Option<String>,
    files: Option<Vec<String>>,
}

/// OpenCode Part 时间
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PartTime {
    start: Option<i64>,
    end: Option<i64>,
}

/// OpenCode Tool 状态
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ToolState {
    status: Option<String>,
    input: Option<serde_json::Value>,
    output: Option<serde_json::Value>,
}

/// 格式化时间戳为字符串
fn format_timestamp(ts: i64) -> String {
    ts.to_string()
}

/// OpenCode 适配器
pub struct OpenCodeAdapter {
    /// OpenCode 存储目录
    data_path: PathBuf,
}

impl Default for OpenCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenCodeAdapter {
    /// 创建适配器（使用默认路径或环境变量）
    pub fn new() -> Self {
        let path = std::env::var(OPENCODE_META.env_var)
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_default()
                    .join(OPENCODE_META.default_path)
            });
        Self { data_path: path }
    }

    /// 使用自定义路径创建适配器
    pub fn with_path(path: PathBuf) -> Self {
        Self { data_path: path }
    }

    /// session 目录路径
    fn session_dir(&self) -> PathBuf {
        self.data_path.join("storage/session")
    }

    /// message 目录路径
    fn message_dir(&self) -> PathBuf {
        self.data_path.join("storage/message")
    }

    /// part 目录路径
    fn part_dir(&self) -> PathBuf {
        self.data_path.join("storage/part")
    }

    /// 从路径提取项目名（跨平台）
    fn extract_project_name(path: &str) -> String {
        std::path::Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path)
            .to_string()
    }

    /// 从 parts 提取内容
    fn extract_content_from_parts(parts: &[PartJson], _msg_data: &MessageJson) -> ParsedContent {
        let mut text_parts = Vec::new();
        let mut full_parts = Vec::new();

        for part in parts {
            match part.type_.as_str() {
                "text" => {
                    text_parts.push(part.text.clone().unwrap_or_default());
                    full_parts.push(part.text.clone().unwrap_or_default());
                }
                "tool" => {
                    let tool_name = part.tool.clone();
                    if let Some(state) = &part.state
                        && state.input.is_some()
                    {
                        full_parts.push(format!(
                            "[Tool: {}]",
                            tool_name.as_deref().unwrap_or("unknown")
                        ));
                    }
                }
                "reasoning" => {
                    full_parts.push(part.text.clone().unwrap_or_default());
                }
                _ => {}
            }
        }

        ParsedContent {
            text: text_parts.join("\n"),
            full: full_parts.join("\n"),
        }
    }
}

impl ConversationAdapter for OpenCodeAdapter {
    fn meta(&self) -> &'static AdapterMeta {
        &OPENCODE_META
    }

    fn data_path(&self) -> &Path {
        &self.data_path
    }

    fn list_sessions(&self) -> Result<Vec<SessionMeta>> {
        let mut results = Vec::new();
        let session_root = self.session_dir();

        if !session_root.exists() {
            tracing::warn!("OpenCode session 目录不存在: {:?}", session_root);
            return Ok(results);
        }

        // 遍历项目目录
        for entry in fs::read_dir(&session_root)? {
            let entry = entry?;
            let project_dir = entry.path();
            if !project_dir.is_dir() {
                continue;
            }

            // 遍历 session 文件
            for session_entry in fs::read_dir(&project_dir)? {
                let session_entry = session_entry?;
                let session_file = session_entry.path();
                let file_name = session_entry.file_name().to_string_lossy().to_string();

                if !file_name.starts_with("ses_") || !file_name.ends_with(".json") {
                    continue;
                }

                // 读取 session JSON
                let content = std::fs::read_to_string(&session_file)?;
                let session_data: SessionJson = serde_json::from_str(&content)?;

                let project_path = session_data.directory.clone();
                let project_name = Self::extract_project_name(&project_path);

                results.push(SessionMeta {
                    id: session_data.id.clone(),
                    source: Source::OpenCode,
                    channel: None,
                    project_path,
                    project_name: Some(project_name),
                    encoded_dir_name: None,
                    session_path: Some(session_file.to_string_lossy().to_string()),
                    file_mtime: None,
                    file_size: None,
                    message_count: None,
                    cwd: None,
                    model: None,
                    meta: None,
                    created_at: Some(format_timestamp(session_data.time.created)),
                    updated_at: Some(format_timestamp(session_data.time.updated)),
                    last_message_type: None,
                    last_message_preview: None,
                    last_message_at: None,
                });
            }
        }

        Ok(results)
    }

    fn parse_session(&self, meta: &SessionMeta) -> Result<Option<ParseResult>> {
        if meta.session_path.is_none() {
            tracing::debug!("OpenCode 会话缺少 session_path: {}", meta.id);
            return Ok(None);
        }

        let session_file = Path::new(meta.session_path.as_ref().unwrap());
        if !session_file.exists() {
            tracing::debug!("OpenCode session 文件不存在: {:?}", session_file);
            return Ok(None);
        }

        // 读取 session JSON
        let content = std::fs::read_to_string(session_file)?;
        let session_data: SessionJson = serde_json::from_str(&content)?;

        // 从 session ID 提取 message 目录
        let message_dir = self.message_dir().join(&session_data.id);
        if !message_dir.exists() {
            tracing::debug!("OpenCode message 目录不存在: {:?}", message_dir);
            return Ok(None);
        }

        // 读取所有 message 文件
        let mut messages = Vec::new();
        let first_timestamp = Some(format_timestamp(session_data.time.created));
        let mut last_timestamp = None;

        for msg_entry in fs::read_dir(&message_dir)? {
            let msg_entry = msg_entry?;
            let msg_file = msg_entry.path();
            let file_name = msg_entry.file_name().to_string_lossy().to_string();

            if !file_name.starts_with("msg_") || !file_name.ends_with(".json") {
                continue;
            }

            // 读取 message JSON
            let msg_content = std::fs::read_to_string(&msg_file)?;
            let msg_data: MessageJson = serde_json::from_str(&msg_content)?;

            let msg_time = format_timestamp(msg_data.time.created);
            if last_timestamp.is_none() {
                last_timestamp = Some(msg_time.clone());
            }

            // 读取 parts
            let part_dir = self.part_dir().join(&msg_data.id);
            let mut parts = Vec::new();

            if part_dir.exists() {
                for part_entry in fs::read_dir(&part_dir)? {
                    let part_entry = part_entry?;
                    let part_file = part_entry.path();
                    let part_file_name = part_file
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !part_file_name.ends_with(".json") {
                        continue;
                    }

                    let part_content = match std::fs::read_to_string(&part_file) {
                        Ok(content) => content,
                        Err(e) => {
                            tracing::warn!("跳过无法读取的 part 文件 {:?}: {}", part_file, e);
                            continue;
                        }
                    };
                    let part_data: PartJson = match serde_json::from_str(&part_content) {
                        Ok(data) => data,
                        Err(e) => {
                            tracing::warn!("跳过损坏的 part 文件 {:?}: {}", part_file, e);
                            continue;
                        }
                    };
                    parts.push(part_data);
                }
            }

            // 构造 ParsedMessage
            let parsed_msg = ParsedMessage {
                uuid: msg_data.id.clone(),
                session_id: meta.id.clone(),
                message_type: match msg_data.role.as_str() {
                    "user" => MessageType::User,
                    "assistant" => MessageType::Assistant,
                    _ => return Ok(None),
                },
                content: Self::extract_content_from_parts(&parts, &msg_data),
                timestamp: Some(msg_time.clone()),
                source: Source::OpenCode,
                channel: None,
                model: msg_data.model_id.clone(),
                tool_call_id: None,
                tool_name: None,
                tool_args: None,
                raw: None,
                cwd: msg_data.path.as_ref().and_then(|p| p.cwd.clone()),
                stop_reason: msg_data.finish.clone(),
            };

            messages.push(parsed_msg);
        }

        Ok(Some(ParseResult {
            messages,
            created_at: first_timestamp,
            updated_at: last_timestamp,
            cwd: Some(session_data.directory.clone()),
            model: None,
            meta: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_timestamp() {
        let ts = 1768452489295;
        let result = format_timestamp(ts);
        assert_eq!(result, "1768452489295");
    }
}
