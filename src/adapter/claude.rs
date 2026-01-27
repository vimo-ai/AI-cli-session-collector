//! Claude Code 数据适配器
//!
//! 解析 ~/.claude/projects/{encoded-path}/{session-uuid}.jsonl

use anyhow::{Context, Result};
use chrono::DateTime;
use serde::Deserialize;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::{AdapterMeta, ConversationAdapter, IncrementalAdapter, IncrementalParseResult};
use crate::domain::{
    IndexableMessage, IndexableSession, MessageType, ParseResult, ParsedContent, ParsedMessage,
    SessionMeta, Source,
};
use crate::incremental::{JsonlIncrementalReader, ReaderState};

// ============================================================================
// 适配器元信息（静态配置）
// ============================================================================

/// Claude Code 适配器元信息
pub static CLAUDE_META: AdapterMeta = AdapterMeta {
    source: Source::Claude,
    name: "Claude Code",
    default_path: ".claude/projects",
    env_var: "CLAUDE_PROJECTS_PATH",
    extensions: &["jsonl"],
    recursive: true,
};

/// 将 ISO 8601 时间戳转换为毫秒时间戳字符串
///
/// 输入: "2026-01-21T06:51:29.630Z"
/// 输出: Some("1769019089630")
fn format_timestamp(ts: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.timestamp_millis().to_string())
}

// ============================================================================
// 适配器实现
// ============================================================================

/// Claude Code 适配器
pub struct ClaudeAdapter {
    /// Claude projects 目录路径
    data_path: PathBuf,
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeAdapter {
    /// 创建适配器（使用默认路径或环境变量）
    pub fn new() -> Self {
        let path = std::env::var(CLAUDE_META.env_var)
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_default()
                    .join(CLAUDE_META.default_path)
            });
        Self { data_path: path }
    }

    /// 使用自定义路径创建适配器
    pub fn with_path(path: PathBuf) -> Self {
        Self { data_path: path }
    }

    /// 从路径提取项目名（跨平台）
    pub fn extract_project_name(path: &str) -> String {
        std::path::Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path)
            .to_string()
    }

    /// 从 JSONL 文件路径直接解析会话（用于索引）
    ///
    /// 从 JSONL 读取 cwd 获取真实项目路径
    ///
    /// # Arguments
    /// * `jsonl_path` - JSONL 文件完整路径
    ///
    /// # Returns
    /// * `Ok(Some(IndexableSession))` - 解析成功
    /// * `Ok(None)` - 文件为空、无有效消息或无 cwd（空会话）
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

        // 2. 从 JSONL 读取 cwd
        let cwd = match Self::read_cwd_from_jsonl(path) {
            Some(cwd) => cwd,
            None => {
                // 无 cwd 表示空会话（没有 user 消息），跳过
                tracing::debug!("空会话（无 cwd）: {}", jsonl_path);
                return Ok(None);
            }
        };

        // 3. 确定 project_path 和 project_name
        let project_path = cwd;
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

            // 全量采集：处理所有类型（跳过无意义的类型）
            let entry_type = entry.entry_type.as_deref();
            let Some(entry_type_str) = entry_type else {
                continue;
            };

            // 跳过无意义的类型：
            // - file-history-snapshot: 悬空的备份文件引用
            // - progress: bash 执行的实时输出快照（每秒一条，最终结果在 tool_result）
            // - queue-operation: 内部消息队列操作日志
            if matches!(entry_type_str, "file-history-snapshot" | "progress" | "queue-operation") {
                continue;
            }

            // 提取 UUID：有则用，无则生成确定性 UUID
            let uuid = entry
                .uuid
                .clone()
                .or_else(|| entry.message.as_ref()?.id.clone())
                .unwrap_or_else(|| {
                    let name = format!(
                        "{}:{}:{}:{}",
                        session_id,
                        entry.timestamp.as_deref().unwrap_or(""),
                        entry_type_str,
                        &line
                    );
                    Uuid::new_v5(&Uuid::NAMESPACE_DNS, name.as_bytes()).to_string()
                });

            // 提取角色（全量采集：其他类型作为 system）
            let role = match entry_type_str {
                "user" => "user",
                "assistant" => "assistant",
                "message" => entry
                    .message
                    .as_ref()
                    .and_then(|m| m.role.as_deref())
                    .unwrap_or("user"),
                _ => "system",
            };

            // 提取内容（根据类型从不同字段提取）
            let content = match entry_type_str {
                "user" | "assistant" | "message" => {
                    let c = Self::extract_content_static(&entry);
                    if c.full.is_empty() {
                        continue;
                    }
                    c
                }
                "summary" => {
                    let text = entry.summary.clone().unwrap_or_default();
                    ParsedContent {
                        text: text.clone(),
                        full: format!("[Summary] {}", text),
                    }
                }
                "custom-title" => {
                    let title = entry.custom_title.clone().unwrap_or_default();
                    ParsedContent {
                        text: title.clone(),
                        full: format!("[custom-title] {}", title),
                    }
                }
                _ => {
                    // system 等有 data 字段的类型
                    let data_str = entry
                        .data
                        .as_ref()
                        .map(|v| serde_json::to_string(v).unwrap_or_default())
                        .unwrap_or_default();
                    let subtype = entry.subtype.as_deref().unwrap_or("");
                    let label = if subtype.is_empty() {
                        entry_type_str.to_string()
                    } else {
                        format!("{}:{}", entry_type_str, subtype)
                    };
                    ParsedContent {
                        text: String::new(),
                        full: format!("[{}] {}", label, data_str),
                    }
                }
            };

            // 解析时间戳
            let timestamp = entry
                .timestamp
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
                raw: Some(line.clone()),
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

    /// 静态方法提取内容（返回分离的 text 和 full）
    fn extract_content_static(entry: &JsonlEntry) -> ParsedContent {
        let Some(message) = &entry.message else {
            return ParsedContent::default();
        };
        let Some(content) = &message.content else {
            return ParsedContent::default();
        };

        match content {
            ContentValue::Text(text) => ParsedContent {
                text: text.clone(),
                full: text.clone(),
            },
            ContentValue::Blocks(blocks) => {
                let mut text_parts = Vec::new();
                let mut full_parts = Vec::new();

                for block in blocks {
                    let (text_part, full_part) = Self::format_content_block(block);
                    if let Some(t) = text_part {
                        text_parts.push(t);
                    }
                    if let Some(f) = full_part {
                        full_parts.push(f);
                    }
                }

                ParsedContent {
                    text: text_parts.join("\n"),
                    full: full_parts.join("\n"),
                }
            }
        }
    }

    /// 格式化单个 content block
    /// 返回 (text_part, full_part)
    /// - text_part: 纯对话文本（用于向量化）
    /// - full_part: 完整格式化内容（用于 FTS）
    fn format_content_block(block: &ContentBlock) -> (Option<String>, Option<String>) {
        match block.block_type.as_deref() {
            Some("text") => {
                // text 类型：两者相同
                (block.text.clone(), block.text.clone())
            }
            Some("tool_use") => {
                // tool_use: 只在 full 中，不参与向量化
                let name = block.name.as_deref().unwrap_or("unknown");
                let input = block
                    .input
                    .as_ref()
                    .map(|v| serde_json::to_string(v).unwrap_or_default())
                    .unwrap_or_default();
                (None, Some(format!("[Tool: {}] {}", name, input)))
            }
            Some("tool_result") => {
                // tool_result: 只在 full 中，不参与向量化
                let error_tag = if block.is_error == Some(true) {
                    " Error"
                } else {
                    ""
                };
                let content = match &block.content {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(v) => serde_json::to_string(v).unwrap_or_default(),
                    None => String::new(),
                };
                (None, Some(format!("[Result{}] {}", error_tag, content)))
            }
            Some("thinking") => {
                // thinking: 全量采集，放入 full
                (None, block.thinking.clone().map(|t| format!("[Thinking] {}", t)))
            }
            _ => (None, None),
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

    /// 从 JSONL 文件提取 cwd
    ///
    /// 策略：找第一条 type="user" 的消息，读取其 cwd 字段
    /// 所有 user 消息都包含 cwd，且 cwd 值在同一会话中保持一致
    fn read_cwd_from_jsonl(file_path: &Path) -> Option<String> {
        let file = File::open(file_path).ok()?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line.ok()?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<JsonlEntry>(&line) {
                // 找到第一条 type="user" 的消息
                if entry.entry_type.as_deref() == Some("user") {
                    return entry.cwd;
                }
            }
        }
        None
    }

    /// 快速统计 JSONL 文件的行数（消息数量）
    /// 只统计非空行，不解析 JSON 内容以提高性能
    fn count_jsonl_lines(file_path: &Path) -> Option<usize> {
        let file = File::open(file_path).ok()?;
        let reader = BufReader::new(file);
        let count = reader
            .lines()
            .map_while(|l| l.ok())
            .filter(|l| !l.trim().is_empty())
            .count();
        Some(count)
    }

    /// 解析单个 JSONL 文件
    fn parse_jsonl_file(&self, file_path: &Path, session_id: &str) -> Result<ParseResult> {
        let file =
            File::open(file_path).with_context(|| format!("无法打开文件: {:?}", file_path))?;
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
                    if let Some(msg) = self.convert_entry(&entry, session_id, &line) {
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

    /// 转换单条消息（全量采集：处理所有消息类型）
    fn convert_entry(
        &self,
        entry: &JsonlEntry,
        session_id: &str,
        raw_line: &str,
    ) -> Option<ParsedMessage> {
        let entry_type = entry.entry_type.as_deref()?;

        // 确定消息类型
        let msg_type = self.get_message_type(entry)?;

        // 获取 UUID：有则用，无则基于内容生成确定性 UUID
        let uuid = entry.uuid.clone().unwrap_or_else(|| {
            // 用 session_id + timestamp + type + raw_line 生成确定性 uuid
            let name = format!(
                "{}:{}:{}:{}",
                session_id,
                entry.timestamp.as_deref().unwrap_or(""),
                entry_type,
                raw_line
            );
            // 使用 DNS namespace 作为基础（也可以自定义）
            Uuid::new_v5(&Uuid::NAMESPACE_DNS, name.as_bytes()).to_string()
        });

        // 提取内容：根据消息类型从不同字段提取
        let (content, tool_call_id, tool_name, tool_args) = match entry_type {
            // 有 message 字段的类型
            "user" | "assistant" | "message" => {
                if let Some(extracted) = self.extract_content(entry) {
                    (
                        extracted.content,
                        extracted.tool_call_id,
                        extracted.tool_name,
                        extracted.tool_args,
                    )
                } else {
                    // message 为空时使用 raw
                    (
                        ParsedContent {
                            text: String::new(),
                            full: format!("[{}] {}", entry_type, raw_line),
                        },
                        None,
                        None,
                        None,
                    )
                }
            }
            // summary 类型
            "summary" => {
                let text = entry.summary.clone().unwrap_or_default();
                (
                    ParsedContent {
                        text: text.clone(),
                        full: format!("[Summary] {}", text),
                    },
                    None,
                    None,
                    None,
                )
            }
            // custom-title 类型
            "custom-title" => {
                let title = entry.custom_title.clone().unwrap_or_default();
                (
                    ParsedContent {
                        text: title.clone(),
                        full: format!("[custom-title] {}", title),
                    },
                    None,
                    None,
                    None,
                )
            }
            // system 等有 data 字段的类型
            _ => {
                let data_str = entry
                    .data
                    .as_ref()
                    .map(|v| serde_json::to_string(v).unwrap_or_default())
                    .unwrap_or_default();
                let subtype = entry.subtype.as_deref().unwrap_or("");
                let label = if subtype.is_empty() {
                    entry_type.to_string()
                } else {
                    format!("{}:{}", entry_type, subtype)
                };
                (
                    ParsedContent {
                        text: String::new(),
                        full: format!("[{}] {}", label, data_str),
                    },
                    None,
                    None,
                    None,
                )
            }
        };

        // 允许空内容（全量采集）
        Some(ParsedMessage {
            uuid,
            session_id: session_id.to_string(),
            message_type: msg_type,
            content,
            timestamp: entry.timestamp.as_deref().and_then(format_timestamp),
            source: Source::Claude,
            channel: Some("code".to_string()),
            model: entry.model.clone(),
            tool_call_id,
            tool_name,
            tool_args,
            raw: Some(raw_line.to_string()),
            cwd: entry.cwd.clone(),
            stop_reason: None,
        })
    }

    /// 获取消息类型（全量采集：处理所有类型）
    fn get_message_type(&self, entry: &JsonlEntry) -> Option<MessageType> {
        let entry_type = entry.entry_type.as_deref()?;

        match entry_type {
            "user" => Some(MessageType::User),
            "assistant" => Some(MessageType::Assistant),
            "message" => {
                let role = entry.message.as_ref()?.role.as_deref()?;
                match role {
                    "assistant" => Some(MessageType::Assistant),
                    "user" => Some(MessageType::User),
                    _ => Some(MessageType::System), // 其他 role 作为 System
                }
            }
            // 跳过无意义的类型
            "file-history-snapshot" | "progress" | "queue-operation" => None,
            // 有价值的元数据类型作为 System
            "summary" | "system" => Some(MessageType::System),
            _ => Some(MessageType::System), // 未知类型也采集
        }
    }

    /// 判断 User 消息是否应该显示（全量采集：始终返回 true）
    #[allow(dead_code)]
    fn should_display_user_message(&self, _entry: &JsonlEntry) -> bool {
        // 全量采集：不过滤任何消息
        true
    }

    /// 检查内容中是否包含 tool_result
    fn has_tool_result_in_content(&self, entry: &JsonlEntry) -> bool {
        if let Some(message) = &entry.message
            && let Some(ContentValue::Blocks(blocks)) = &message.content
        {
            for block in blocks {
                if block.block_type.as_deref() == Some("tool_result") {
                    return true;
                }
            }
        }
        false
    }

    /// 提取消息内容（返回分离的 text 和 full 以及工具信息）
    fn extract_content(&self, entry: &JsonlEntry) -> Option<ExtractedContent> {
        let message = entry.message.as_ref()?;
        let content = message.content.as_ref()?;

        match content {
            ContentValue::Text(text) => {
                if text.is_empty() {
                    None
                } else {
                    Some(ExtractedContent {
                        content: ParsedContent {
                            text: text.clone(),
                            full: text.clone(),
                        },
                        tool_call_id: None,
                        tool_name: None,
                        tool_args: None,
                    })
                }
            }
            ContentValue::Blocks(blocks) => {
                let mut text_parts = Vec::new();
                let mut full_parts = Vec::new();
                let mut tool_call_id = None;
                let mut tool_name = None;
                let mut tool_args = None;

                for block in blocks {
                    let (text_part, full_part) = Self::format_content_block(block);
                    if let Some(t) = text_part {
                        text_parts.push(t);
                    }
                    if let Some(f) = full_part {
                        full_parts.push(f);
                    }

                    // 提取 tool_use block 的信息
                    if block.block_type.as_deref() == Some("tool_use") {
                        tool_call_id = block.id.clone();
                        tool_name = block.name.clone();
                        tool_args = block
                            .input
                            .as_ref()
                            .map(|v| serde_json::to_string(v).unwrap_or_default());
                    }
                }

                // 如果 full 为空，返回 None
                if full_parts.is_empty() {
                    None
                } else {
                    Some(ExtractedContent {
                        content: ParsedContent {
                            text: text_parts.join("\n"),
                            full: full_parts.join("\n"),
                        },
                        tool_call_id,
                        tool_name,
                        tool_args,
                    })
                }
            }
        }
    }
}

impl ConversationAdapter for ClaudeAdapter {
    fn meta(&self) -> &'static AdapterMeta {
        &CLAUDE_META
    }

    fn data_path(&self) -> &Path {
        &self.data_path
    }

    fn list_sessions(&self) -> Result<Vec<SessionMeta>> {
        let mut results = Vec::new();

        if !self.data_path.exists() {
            tracing::warn!("Claude projects 目录不存在: {:?}", self.data_path);
            return Ok(results);
        }

        // 遍历项目目录
        for entry in fs::read_dir(&self.data_path)? {
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

                // 快速统计消息数量（JSONL 行数）
                let message_count = Self::count_jsonl_lines(&file_path);

                // project_path 使用 cwd，空会话时留空（由调用方处理 fallback）
                let project_path = cwd.clone().unwrap_or_default();
                let project_name = if project_path.is_empty() {
                    None
                } else {
                    Some(Self::extract_project_name(&project_path))
                };

                results.push(SessionMeta {
                    id: session_id.to_string(),
                    source: Source::Claude,
                    channel: Some("code".to_string()),
                    project_path,
                    project_name,
                    encoded_dir_name: Some(encoded_name.to_string()),
                    session_path: Some(file_path.to_string_lossy().to_string()),
                    file_mtime,
                    file_size,
                    message_count,
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

impl IncrementalAdapter for ClaudeAdapter {
    fn parse_session_incremental(
        &self,
        meta: &SessionMeta,
        state: Option<ReaderState>,
    ) -> Result<IncrementalParseResult> {
        let session_path = match &meta.session_path {
            Some(p) => p,
            None => {
                tracing::warn!("缺少 session_path: {}", meta.id);
                return Ok(IncrementalParseResult::default());
            }
        };

        let path = Path::new(session_path);
        if !path.exists() {
            tracing::debug!("会话文件不存在（可能已过期）: {}", session_path);
            return Ok(IncrementalParseResult::default());
        }

        // 使用增量读取器读取新增的行
        let reader = JsonlIncrementalReader::new();
        let (lines, stats, new_state) = reader.read_incremental(path, state)?;

        if lines.is_empty() {
            return Ok(IncrementalParseResult {
                result: None,
                state: new_state,
                was_reset: stats.was_reset,
            });
        }

        // 解析新增的行
        let mut messages = Vec::new();
        let mut cwd = None;
        let mut model = None;
        let mut first_timestamp = None;
        let mut last_timestamp = None;

        for line in &lines {
            match serde_json::from_str::<JsonlEntry>(line) {
                Ok(entry) => {
                    // 提取会话元数据
                    if cwd.is_none() && entry.cwd.is_some() {
                        cwd = entry.cwd.clone();
                    }
                    if model.is_none() && entry.model.is_some() {
                        model = entry.model.clone();
                    }

                    // 转换消息
                    if let Some(msg) = self.convert_entry(&entry, &meta.id, line) {
                        if first_timestamp.is_none() {
                            first_timestamp = msg.timestamp.clone();
                        }
                        last_timestamp = msg.timestamp.clone();
                        messages.push(msg);
                    }
                }
                Err(e) => {
                    tracing::debug!("解析失败: {}", e);
                }
            }
        }

        let result = if messages.is_empty() {
            None
        } else {
            Some(ParseResult {
                messages,
                created_at: first_timestamp,
                updated_at: last_timestamp,
                cwd,
                model,
                meta: None,
            })
        };

        Ok(IncrementalParseResult {
            result,
            state: new_state,
            was_reset: stats.was_reset,
        })
    }
}

// ==================== JSONL 数据结构 ====================

/// 提取的内容（包括工具信息）
struct ExtractedContent {
    content: ParsedContent,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    tool_args: Option<String>,
}

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
    // 全量采集：支持更多消息类型
    summary: Option<String>,           // summary 类型的内容
    data: Option<serde_json::Value>,   // progress/system 等类型的数据
    subtype: Option<String>,           // system 类型的子类型
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,      // custom-title 类型的内容
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
#[allow(dead_code)] // 字段用于 serde 反序列化
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: Option<String>,
    // text block
    text: Option<String>,
    // tool_use block
    id: Option<String>,
    name: Option<String>,
    input: Option<serde_json::Value>,
    // tool_result block
    tool_use_id: Option<String>,
    content: Option<serde_json::Value>,
    is_error: Option<bool>,
    // thinking block
    thinking: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_cwd_from_jsonl_user_message() {
        // 创建临时文件，模拟真实的 JSONL 格式
        let mut file = NamedTempFile::new().unwrap();

        // 写入模拟数据：summary 消息没有 cwd，user 消息有 cwd
        writeln!(file, r#"{{"uuid":"0001","type":"summary","message":{{"role":"assistant","content":"summary"}}}}"#).unwrap();
        writeln!(file, r#"{{"uuid":"0002","type":"user","cwd":"/Users/test/project","message":{{"role":"user","content":"hello"}}}}"#).unwrap();
        writeln!(file, r#"{{"uuid":"0003","type":"assistant","message":{{"role":"assistant","content":"hi"}}}}"#).unwrap();
        file.flush().unwrap();

        let cwd = ClaudeAdapter::read_cwd_from_jsonl(file.path());
        assert_eq!(cwd, Some("/Users/test/project".to_string()));
    }

    #[test]
    fn test_read_cwd_from_jsonl_user_first_line() {
        // user 消息在第一行
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"uuid":"0001","type":"user","cwd":"/home/user/code","message":{{"role":"user","content":"test"}}}}"#).unwrap();
        file.flush().unwrap();

        let cwd = ClaudeAdapter::read_cwd_from_jsonl(file.path());
        assert_eq!(cwd, Some("/home/user/code".to_string()));
    }

    #[test]
    fn test_read_cwd_from_jsonl_no_user_message() {
        // 没有 user 消息
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"uuid":"0001","type":"summary","message":{{"role":"assistant","content":"test"}}}}"#).unwrap();
        writeln!(file, r#"{{"uuid":"0002","type":"assistant","message":{{"role":"assistant","content":"test2"}}}}"#).unwrap();
        file.flush().unwrap();

        let cwd = ClaudeAdapter::read_cwd_from_jsonl(file.path());
        assert_eq!(cwd, None);
    }

    #[test]
    fn test_read_cwd_from_jsonl_empty_file() {
        let file = NamedTempFile::new().unwrap();
        let cwd = ClaudeAdapter::read_cwd_from_jsonl(file.path());
        assert_eq!(cwd, None);
    }

    #[test]
    fn test_read_cwd_from_jsonl_user_without_cwd() {
        // user 消息存在但没有 cwd 字段（理论上不应该发生）
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"uuid":"0001","type":"user","message":{{"role":"user","content":"hello"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();

        let cwd = ClaudeAdapter::read_cwd_from_jsonl(file.path());
        assert_eq!(cwd, None);
    }

    #[test]
    fn test_extract_project_name() {
        assert_eq!(
            ClaudeAdapter::extract_project_name("/Users/xxx/project"),
            "project"
        );
        assert_eq!(
            ClaudeAdapter::extract_project_name("/a/b/c/deep/path"),
            "path"
        );
        assert_eq!(ClaudeAdapter::extract_project_name("/single"), "single");
        assert_eq!(ClaudeAdapter::extract_project_name("nopath"), "nopath");
    }
}
