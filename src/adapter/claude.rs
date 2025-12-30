//! Claude Code 数据适配器
//!
//! 解析 ~/.claude/projects/{encoded-path}/{session-uuid}.jsonl

use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::ConversationAdapter;
use crate::domain::{IndexableMessage, IndexableSession, MessageType, ParseResult, ParsedMessage, SessionMeta, Source};

/// Claude Code 适配器
pub struct ClaudeAdapter {
    /// Claude projects 目录路径
    projects_path: PathBuf,
}

impl ClaudeAdapter {
    /// 创建适配器
    pub fn new(projects_path: PathBuf) -> Self {
        Self { projects_path }
    }

    /// 解码 Claude Code 目录名为真实路径
    /// Claude Code 使用 `-` 替换 `/` 的编码方式
    /// @example -Users-xxx-project → /Users/xxx/project
    pub fn decode_path(encoded: &str) -> String {
        // 先移除开头的 `-`，替换为 `/`，然后把所有 `-` 替换为 `/`
        let decoded = if encoded.starts_with('-') {
            format!("/{}", &encoded[1..].replace('-', "/"))
        } else {
            encoded.replace('-', "/")
        };
        decoded
    }

    /// 从路径提取项目名
    pub fn extract_project_name(path: &str) -> String {
        path.split('/')
            .filter(|s| !s.is_empty())
            .last()
            .unwrap_or(path)
            .to_string()
    }

    /// 从 JSONL 文件路径直接解析会话（用于索引）
    ///
    /// 这是最核心的方法，正确处理中文路径问题：
    /// 1. 从 JSONL 读取 cwd 获取真实项目路径
    /// 2. 如果没有 cwd，回退到 decode_path
    ///
    /// # Arguments
    /// * `jsonl_path` - JSONL 文件完整路径
    ///
    /// # Returns
    /// * `Ok(Some(IndexableSession))` - 解析成功
    /// * `Ok(None)` - 文件为空或无有效消息
    /// * `Err` - 解析失败
    pub fn parse_session_from_path(jsonl_path: &str) -> Result<Option<IndexableSession>> {
        let path = Path::new(jsonl_path);
        if !path.exists() {
            anyhow::bail!("文件不存在: {}", jsonl_path);
        }

        // 1. 从路径提取 session_id
        let session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("无效的文件名"))?;

        // 2. 从路径提取编码的目录名
        let encoded_dir_name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        // 3. 从 JSONL 读取 cwd（优先）
        let cwd = Self::read_cwd_from_jsonl(path);

        // 4. 确定正确的 project_path
        let project_path = cwd.unwrap_or_else(|| Self::decode_path(encoded_dir_name));
        let project_name = Self::extract_project_name(&project_path);

        // 5. 解析消息
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut messages = Vec::new();
        let mut sequence: i64 = 0;

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) else {
                continue;
            };

            // 只处理 message 类型
            let entry_type = entry.entry_type.as_deref();
            if entry_type != Some("message") && entry_type != Some("user") && entry_type != Some("assistant") {
                continue;
            }

            // 提取 UUID
            let Some(uuid) = entry.uuid.clone().or_else(|| {
                entry.message.as_ref()?.id.clone()
            }) else {
                continue;
            };

            // 提取角色
            let role = match entry_type {
                Some("user") => "user",
                Some("assistant") => "assistant",
                Some("message") => {
                    entry.message.as_ref()
                        .and_then(|m| m.role.as_deref())
                        .unwrap_or("user")
                }
                _ => continue,
            };

            // 提取内容
            let content = Self::extract_content_static(&entry);
            if content.is_empty() {
                continue;
            }

            // 解析时间戳
            let timestamp = entry.timestamp
                .as_ref()
                .and_then(|s| Self::parse_timestamp(s))
                .unwrap_or_else(|| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0)
                });

            messages.push(IndexableMessage {
                uuid,
                role: role.to_string(),
                content,
                timestamp,
                sequence,
            });
            sequence += 1;
        }

        if messages.is_empty() {
            return Ok(None);
        }

        Ok(Some(IndexableSession {
            session_id: session_id.to_string(),
            project_path,
            project_name,
            messages,
        }))
    }

    /// 静态方法提取内容
    fn extract_content_static(entry: &JsonlEntry) -> String {
        let Some(message) = &entry.message else {
            return String::new();
        };
        let Some(content) = &message.content else {
            return String::new();
        };

        match content {
            ContentValue::Text(text) => text.clone(),
            ContentValue::Blocks(blocks) => {
                blocks.iter()
                    .filter_map(|b| {
                        if b.block_type.as_deref() == Some("text") {
                            b.text.as_deref()
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
    }

    /// 解析 ISO8601 时间戳为毫秒
    fn parse_timestamp(s: &str) -> Option<i64> {
        // 尝试解析 ISO8601 格式
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return Some(dt.timestamp_millis());
        }
        // 尝试解析纯数字（已经是毫秒）
        if let Ok(ms) = s.parse::<i64>() {
            return Some(ms);
        }
        None
    }

    /// 从 JSONL 文件快速读取 cwd（只读前几行）
    fn read_cwd_from_jsonl(file_path: &Path) -> Option<String> {
        let file = File::open(file_path).ok()?;
        let reader = BufReader::new(file);

        // 只读取前 10 行寻找 cwd
        for line in reader.lines().take(10) {
            if let Ok(line) = line {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
                    if let Some(cwd) = entry.cwd {
                        return Some(cwd);
                    }
                }
            }
        }
        None
    }

    /// 解析单个 JSONL 文件
    fn parse_jsonl_file(
        &self,
        file_path: &Path,
        session_id: &str,
    ) -> Result<ParseResult> {
        let file = File::open(file_path)
            .with_context(|| format!("无法打开文件: {:?}", file_path))?;
        let reader = BufReader::new(file);

        let mut messages = Vec::new();
        let mut cwd = None;
        let mut model = None;
        let mut first_timestamp = None;
        let mut last_timestamp = None;

        for (line_num, line) in reader.lines().enumerate() {
            let line = line.with_context(|| format!("读取第 {} 行失败", line_num + 1))?;
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<JsonlEntry>(&line) {
                Ok(entry) => {
                    // 提取会话元数据
                    if cwd.is_none() && entry.cwd.is_some() {
                        cwd = entry.cwd.clone();
                    }
                    if model.is_none() && entry.model.is_some() {
                        model = entry.model.clone();
                    }

                    // 转换消息
                    if let Some(msg) = self.convert_entry(&entry, session_id) {
                        if first_timestamp.is_none() {
                            first_timestamp = msg.timestamp.clone();
                        }
                        last_timestamp = msg.timestamp.clone();
                        messages.push(msg);
                    }
                }
                Err(e) => {
                    tracing::debug!("第 {} 行解析失败: {}", line_num + 1, e);
                }
            }
        }

        Ok(ParseResult {
            messages,
            created_at: first_timestamp,
            updated_at: last_timestamp,
            cwd,
            model,
            meta: None,
        })
    }

    /// 转换单条消息
    fn convert_entry(&self, entry: &JsonlEntry, session_id: &str) -> Option<ParsedMessage> {
        let entry_type = entry.entry_type.as_deref()?;

        // 跳过 summary 类型
        if entry_type == "summary" {
            return None;
        }

        // 确定消息类型
        let msg_type = self.get_message_type(entry)?;

        // 提取内容
        let content = self.extract_content(entry)?;
        if content.is_empty() {
            return None;
        }

        // 获取 UUID
        let uuid = entry
            .uuid
            .clone()
            .or_else(|| entry.message.as_ref()?.id.clone())?;

        Some(ParsedMessage {
            uuid,
            session_id: session_id.to_string(),
            message_type: msg_type,
            content,
            timestamp: entry.timestamp.clone(),
            source: Source::Claude,
            channel: Some("code".to_string()),
            model: entry.model.clone(),
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            raw: None,
        })
    }

    /// 获取消息类型
    fn get_message_type(&self, entry: &JsonlEntry) -> Option<MessageType> {
        let entry_type = entry.entry_type.as_deref()?;

        match entry_type {
            "user" => {
                if !self.should_display_user_message(entry) {
                    return None;
                }
                Some(MessageType::User)
            }
            "assistant" => Some(MessageType::Assistant),
            "message" => {
                let role = entry.message.as_ref()?.role.as_deref()?;
                match role {
                    "assistant" => Some(MessageType::Assistant),
                    "user" => {
                        if !self.should_display_user_message(entry) {
                            return None;
                        }
                        Some(MessageType::User)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// 判断 User 消息是否应该显示
    fn should_display_user_message(&self, entry: &JsonlEntry) -> bool {
        // 工具执行结果 - 不显示
        if entry.tool_use_result.is_some() {
            return false;
        }

        // 检查 content 中是否包含 tool_result
        if self.has_tool_result_in_content(entry) {
            return false;
        }

        // 仅 Transcript 可见 - 不显示
        if entry.is_visible_in_transcript_only == Some(true) {
            return false;
        }

        // 压缩摘要 - 不显示
        if entry.is_compact_summary == Some(true) {
            return false;
        }

        // 元数据消息 - 不显示
        if entry.is_meta == Some(true) {
            return false;
        }

        true
    }

    /// 检查内容中是否包含 tool_result
    fn has_tool_result_in_content(&self, entry: &JsonlEntry) -> bool {
        if let Some(message) = &entry.message {
            if let Some(ContentValue::Blocks(blocks)) = &message.content {
                for block in blocks {
                    if block.block_type.as_deref() == Some("tool_result") {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 提取消息内容
    fn extract_content(&self, entry: &JsonlEntry) -> Option<String> {
        let message = entry.message.as_ref()?;
        let content = message.content.as_ref()?;

        match content {
            ContentValue::Text(text) => {
                if text.is_empty() {
                    None
                } else {
                    Some(text.clone())
                }
            }
            ContentValue::Blocks(blocks) => {
                let text_parts: Vec<&str> = blocks
                    .iter()
                    .filter_map(|b| {
                        if b.block_type.as_deref() == Some("text") {
                            b.text.as_deref()
                        } else {
                            None
                        }
                    })
                    .collect();

                if text_parts.is_empty() {
                    None
                } else {
                    Some(text_parts.join("\n"))
                }
            }
        }
    }
}

impl ConversationAdapter for ClaudeAdapter {
    fn source(&self) -> Source {
        Source::Claude
    }

    fn list_sessions(&self) -> Result<Vec<SessionMeta>> {
        let mut results = Vec::new();

        if !self.projects_path.exists() {
            tracing::warn!("Claude projects 目录不存在: {:?}", self.projects_path);
            return Ok(results);
        }

        // 遍历项目目录
        for entry in fs::read_dir(&self.projects_path)? {
            let entry = entry?;
            let project_dir = entry.path();

            if !project_dir.is_dir() {
                continue;
            }

            let encoded_name = project_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();

            if encoded_name.is_empty() || encoded_name.starts_with('.') {
                continue;
            }

            // 解码项目路径（作为 fallback，当 JSONL 中无 cwd 时使用）
            let decoded_path = Self::decode_path(encoded_name);

            // 扫描 JSONL 文件
            for file_entry in fs::read_dir(&project_dir)? {
                let file_entry = file_entry?;
                let file_path = file_entry.path();

                if !file_path.is_file() {
                    continue;
                }

                let file_name = file_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();

                if !file_name.ends_with(".jsonl") {
                    continue;
                }

                let session_id = file_name.trim_end_matches(".jsonl");
                if session_id.is_empty() {
                    continue;
                }

                // 获取文件元数据
                let (file_mtime, file_size) = match fs::metadata(&file_path) {
                    Ok(meta) => {
                        let mtime = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_millis() as u64);
                        (mtime, Some(meta.len()))
                    }
                    Err(_) => (None, None),
                };

                // 从 JSONL 文件读取真实的 cwd（解决非 ASCII 路径问题）
                let cwd = Self::read_cwd_from_jsonl(&file_path);

                // 优先使用 cwd，否则回退到 decode_path 结果
                let actual_project_path = cwd.clone().unwrap_or_else(|| decoded_path.clone());
                let actual_project_name = Self::extract_project_name(&actual_project_path);

                results.push(SessionMeta {
                    id: session_id.to_string(),
                    source: Source::Claude,
                    channel: Some("code".to_string()),
                    project_path: actual_project_path,
                    project_name: Some(actual_project_name),
                    encoded_dir_name: Some(encoded_name.to_string()),
                    session_path: Some(file_path.to_string_lossy().to_string()),
                    file_mtime,
                    file_size,
                    cwd,
                    model: None,
                    meta: None,
                    created_at: None,
                    updated_at: None,
                });
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
            // 文件可能被 Claude Code 清理（30天过期），这是正常现象
            tracing::debug!("会话文件不存在（可能已过期）: {}", session_path);
            return Ok(None);
        }

        let result = self.parse_jsonl_file(path, &meta.id)?;
        Ok(Some(result))
    }
}

// ==================== JSONL 数据结构 ====================

#[derive(Debug, Deserialize)]
struct JsonlEntry {
    uuid: Option<String>,
    #[serde(rename = "type")]
    entry_type: Option<String>,
    message: Option<MessageContent>,
    timestamp: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    #[serde(rename = "toolUseResult")]
    tool_use_result: Option<serde_json::Value>,
    #[serde(rename = "isVisibleInTranscriptOnly")]
    is_visible_in_transcript_only: Option<bool>,
    #[serde(rename = "isCompactSummary")]
    is_compact_summary: Option<bool>,
    #[serde(rename = "isMeta")]
    is_meta: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MessageContent {
    id: Option<String>,
    role: Option<String>,
    content: Option<ContentValue>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ContentValue {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: Option<String>,
    text: Option<String>,
}
