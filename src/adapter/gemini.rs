//! Gemini CLI 数据适配器
//!
//! 解析 ~/.gemini/tmp/{project-hash}/chats/session-*.json
//!
//! ## 数据格式
//! - ~/.gemini/tmp/{project-hash}/logs.json: 简单的用户消息日志
//! - ~/.gemini/tmp/{project-hash}/chats/session-*.json: 完整会话记录
//!
//! ## 会话文件结构
//! ```json
//! {
//!   "sessionId": "uuid",
//!   "projectHash": "hash",
//!   "startTime": "ISO8601",
//!   "lastUpdated": "ISO8601",
//!   "messages": [
//!     {
//!       "id": "uuid",
//!       "timestamp": "ISO8601",
//!       "type": "user" | "gemini",
//!       "content": "message content",
//!       "thoughts": [...],  // 可选，仅 gemini 消息
//!       "model": "gemini-2.5-pro",  // 可选
//!       "toolCalls": [...]   // 可选
//!     }
//!   ]
//! }
//! ```

use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use super::{AdapterMeta, ConversationAdapter};
use crate::domain::{
    MessageType, ParseResult, ParsedContent, ParsedMessage, SessionMeta, Source,
};

// ============================================================================
// 适配器元信息（静态配置）
// ============================================================================

/// Gemini CLI 适配器元信息
pub static GEMINI_META: AdapterMeta = AdapterMeta {
    source: Source::Gemini,
    name: "Gemini CLI",
    default_path: ".gemini/tmp",
    env_var: "GEMINI_TMP_PATH",
    extensions: &["json"],
    recursive: true,
};

// ============================================================================
// JSON 数据结构
// ============================================================================

/// 完整会话文件结构
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionFile {
    session_id: String,
    project_hash: Option<String>,
    start_time: Option<String>,
    last_updated: Option<String>,
    messages: Vec<MessageEntry>,
}

/// 消息条目
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageEntry {
    id: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "type")]
    msg_type: Option<String>,
    content: Option<String>,
    thoughts: Option<Vec<ThoughtEntry>>,
    model: Option<String>,
    tool_calls: Option<Vec<ToolCallEntry>>,
    tokens: Option<TokenInfo>,
}

/// 思考条目（Gemini 的推理过程）
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[allow(dead_code)]
struct ThoughtEntry {
    subject: Option<String>,
    description: Option<String>,
    timestamp: Option<String>,
}

/// 工具调用条目
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[allow(dead_code)]
struct ToolCallEntry {
    id: Option<String>,
    name: Option<String>,
    args: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
    status: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "resultDisplay")]
    result_display: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    description: Option<String>,
}

/// Token 统计信息
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[allow(dead_code)]
struct TokenInfo {
    input: Option<i64>,
    output: Option<i64>,
    cached: Option<i64>,
    thoughts: Option<i64>,
    tool: Option<i64>,
    total: Option<i64>,
}

// ============================================================================
// 适配器实现
// ============================================================================

/// Gemini CLI 适配器
pub struct GeminiAdapter {
    /// Gemini tmp 目录 (~/.gemini/tmp)
    data_path: PathBuf,
}

impl Default for GeminiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl GeminiAdapter {
    /// 创建适配器（使用默认路径或环境变量）
    pub fn new() -> Self {
        let path = std::env::var(GEMINI_META.env_var)
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_default()
                    .join(GEMINI_META.default_path)
            });
        Self { data_path: path }
    }

    /// 使用自定义路径创建适配器
    pub fn with_path(path: PathBuf) -> Self {
        Self { data_path: path }
    }

    /// 扫描项目目录下的所有会话文件
    fn scan_project_sessions(&self, project_dir: &Path) -> Vec<PathBuf> {
        let mut sessions = Vec::new();
        let chats_dir = project_dir.join("chats");

        if chats_dir.exists() && chats_dir.is_dir()
            && let Ok(entries) = fs::read_dir(&chats_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && let Some(name) = path.file_name().and_then(|n| n.to_str())
                {
                    // 匹配 session-*.json 格式
                    if name.starts_with("session-") && name.ends_with(".json") {
                        sessions.push(path);
                    }
                }
            }
        }

        sessions
    }

    /// 从会话文件读取元数据
    fn read_session_meta(&self, session_path: &Path, project_hash: &str) -> Option<SessionMeta> {
        let content = match fs::read_to_string(session_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!("读取会话文件失败 {:?}: {}", session_path, e);
                return None;
            }
        };
        let session: SessionFile = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!("解析会话 JSON 失败 {:?}: {}", session_path, e);
                return None;
            }
        };

        // 获取文件元数据
        let (file_mtime, file_size) = fs::metadata(session_path)
            .map(|m| {
                let mtime = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64);
                (mtime, Some(m.len()))
            })
            .unwrap_or((None, None));

        // 尝试获取项目路径
        // Gemini CLI 没有直接存储 cwd，使用 project_hash 作为标识
        let project_path = format!("gemini:{}", project_hash);
        let project_name = format!("Project-{}", &project_hash[..8.min(project_hash.len())]);

        Some(SessionMeta {
            id: session.session_id.clone(),
            source: Source::Gemini,
            channel: Some("cli".to_string()),
            project_path,
            project_name: Some(project_name),
            encoded_dir_name: Some(project_hash.to_string()),
            session_path: Some(session_path.to_string_lossy().to_string()),
            file_mtime,
            file_size,
            message_count: Some(session.messages.len()),
            cwd: None, // Gemini CLI 不存储 cwd
            model: session
                .messages
                .iter()
                .find_map(|m| m.model.clone()),
            meta: Some(serde_json::json!({
                "projectHash": session.project_hash.as_deref().unwrap_or(project_hash)
            })),
            created_at: session.start_time.clone(),
            updated_at: session.last_updated.clone(),
        })
    }

    /// 解析会话文件
    fn parse_session_file(&self, session_path: &Path) -> Result<ParseResult> {
        let content = fs::read_to_string(session_path)?;
        let session: SessionFile = serde_json::from_str(&content)?;

        let mut messages = Vec::new();
        let mut model: Option<String> = None;
        let mut sequence = 0i64;

        for entry in &session.messages {
            // 提取模型信息
            if model.is_none() && entry.model.is_some() {
                model = entry.model.clone();
            }

            // 判断消息类型
            let msg_type = match entry.msg_type.as_deref() {
                Some("user") => MessageType::User,
                Some("gemini") => MessageType::Assistant,
                // info/error 是系统消息（模型切换、请求取消、API 错误），跳过
                Some("info") | Some("error") => {
                    tracing::trace!("跳过系统消息 type={:?}", entry.msg_type);
                    continue;
                }
                other => {
                    tracing::debug!("跳过未知消息类型: {:?}", other);
                    continue;
                }
            };

            // 提取内容
            let content_text = entry.content.clone().unwrap_or_default();
            if content_text.is_empty() {
                continue;
            }

            // 构建完整内容（包括工具调用）
            let mut full_parts = vec![content_text.clone()];

            // 添加工具调用信息（完整记录所有工具）
            let mut tool_names: Vec<String> = Vec::new();
            let mut tool_args_list: Vec<String> = Vec::new();
            if let Some(tool_calls) = &entry.tool_calls {
                for tc in tool_calls {
                    let name = tc.name.as_deref().unwrap_or("unknown");
                    let display = tc.display_name.as_deref().unwrap_or(name);
                    // 记录工具名
                    tool_names.push(name.to_string());
                    // 记录工具参数
                    if let Some(args) = &tc.args {
                        tool_args_list.push(serde_json::to_string(args).unwrap_or_default());
                    }
                    // 添加到 full_parts（包含参数摘要）
                    let args_preview = tc.args.as_ref()
                        .map(|a| {
                            let s = serde_json::to_string(a).unwrap_or_default();
                            if s.chars().count() > 100 {
                                format!("{}...", s.chars().take(100).collect::<String>())
                            } else {
                                s
                            }
                        })
                        .unwrap_or_default();
                    full_parts.push(format!("[Tool: {}] {}", display, args_preview));
                }
            }

            // 生成消息 UUID
            let uuid = entry.id.clone().unwrap_or_else(|| {
                format!("{}:{}:{}", session.session_id, msg_type, sequence)
            });

            messages.push(ParsedMessage {
                uuid,
                session_id: session.session_id.clone(),
                message_type: msg_type,
                content: ParsedContent {
                    text: content_text, // 纯文本用于向量化
                    full: full_parts.join("\n"), // 完整内容用于 FTS
                },
                timestamp: entry.timestamp.clone(),
                source: Source::Gemini,
                channel: Some("cli".to_string()),
                model: entry.model.clone(),
                tool_call_id: None,
                // 多工具时用逗号连接
                tool_name: if tool_names.is_empty() {
                    None
                } else {
                    Some(tool_names.join(","))
                },
                tool_args: if tool_args_list.is_empty() {
                    None
                } else {
                    Some(tool_args_list.join("\n---\n"))
                },
                raw: Some(serde_json::to_string(&entry).unwrap_or_default()),
                cwd: None,
                stop_reason: None,
            });
            sequence += 1;
        }

        Ok(ParseResult {
            messages,
            created_at: session.start_time,
            updated_at: session.last_updated,
            cwd: None,
            model,
            meta: session.project_hash.map(|h| serde_json::json!({ "projectHash": h })),
        })
    }
}

impl ConversationAdapter for GeminiAdapter {
    fn meta(&self) -> &'static AdapterMeta {
        &GEMINI_META
    }

    fn data_path(&self) -> &Path {
        &self.data_path
    }

    fn list_sessions(&self) -> Result<Vec<SessionMeta>> {
        let mut results = Vec::new();

        if !self.data_path.exists() {
            tracing::warn!("Gemini tmp 目录不存在: {:?}", self.data_path);
            return Ok(results);
        }

        // 遍历项目目录（哈希目录）
        if let Ok(entries) = fs::read_dir(&self.data_path) {
            for entry in entries.flatten() {
                let project_dir = entry.path();

                // 跳过非目录和特殊目录
                if !project_dir.is_dir() {
                    continue;
                }

                let project_hash = match project_dir.file_name().and_then(|n| n.to_str()) {
                    Some(name) if !name.starts_with('.') && name != "bin" => name,
                    _ => continue,
                };

                // 扫描项目下的所有会话文件
                let session_files = self.scan_project_sessions(&project_dir);
                for session_path in session_files {
                    if let Some(meta) = self.read_session_meta(&session_path, project_hash) {
                        results.push(meta);
                    }
                }
            }
        }

        Ok(results)
    }

    fn parse_session(&self, meta: &SessionMeta) -> Result<Option<ParseResult>> {
        let session_path = match &meta.session_path {
            Some(p) => p,
            None => {
                tracing::warn!("缺少 session_path: {}", meta.id);
                return Ok(None);
            }
        };

        let path = Path::new(session_path);
        if !path.exists() {
            tracing::debug!("会话文件不存在: {}", session_path);
            return Ok(None);
        }

        let result = self.parse_session_file(path)?;
        Ok(Some(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_session(dir: &Path, session_id: &str) -> PathBuf {
        let chats_dir = dir.join("chats");
        fs::create_dir_all(&chats_dir).unwrap();

        let session_file = chats_dir.join(format!("session-2025-01-01T00-00-{}.json", session_id));
        let content = serde_json::json!({
            "sessionId": session_id,
            "projectHash": "abc123",
            "startTime": "2025-01-01T00:00:00.000Z",
            "lastUpdated": "2025-01-01T01:00:00.000Z",
            "messages": [
                {
                    "id": "msg-1",
                    "timestamp": "2025-01-01T00:00:00.000Z",
                    "type": "user",
                    "content": "Hello Gemini"
                },
                {
                    "id": "msg-2",
                    "timestamp": "2025-01-01T00:00:10.000Z",
                    "type": "gemini",
                    "content": "Hello! How can I help you today?",
                    "model": "gemini-2.5-pro"
                }
            ]
        });

        let mut file = fs::File::create(&session_file).unwrap();
        file.write_all(serde_json::to_string_pretty(&content).unwrap().as_bytes())
            .unwrap();

        session_file
    }

    #[test]
    fn test_list_sessions() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("abc123hash");
        fs::create_dir_all(&project_dir).unwrap();

        create_test_session(&project_dir, "test-session-1");

        let adapter = GeminiAdapter::with_path(temp_dir.path().to_path_buf());
        let sessions = adapter.list_sessions().unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "test-session-1");
        assert_eq!(sessions[0].source, Source::Gemini);
    }

    #[test]
    fn test_parse_session() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("abc123hash");
        fs::create_dir_all(&project_dir).unwrap();

        create_test_session(&project_dir, "test-session-2");

        let adapter = GeminiAdapter::with_path(temp_dir.path().to_path_buf());
        let sessions = adapter.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);

        let result = adapter.parse_session(&sessions[0]).unwrap().unwrap();
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[0].message_type, MessageType::User);
        assert_eq!(result.messages[0].content.text, "Hello Gemini");
        assert_eq!(result.messages[1].message_type, MessageType::Assistant);
        assert!(result.messages[1]
            .content
            .text
            .contains("Hello! How can I help you today?"));
        assert_eq!(result.model, Some("gemini-2.5-pro".to_string()));
    }

}
