//! Codex CLI 数据适配器
//!
//! 全量采集 ~/.codex/ 下的会话数据
//!
//! ## 数据格式
//! - history.jsonl: 会话摘要列表
//! - sessions/{year}/{month}/{day}/{sessionId}.jsonl: rollout 事件流
//!
//! ## 事件类型
//! - session_meta: 会话元数据 (id, cwd, cli_version, source, git)
//! - turn_context: 对话上下文 (cwd, approval_policy, sandbox_policy, model, summary)
//! - response_item: AI响应项
//!   - message: 用户/助手消息
//!   - reasoning: 推理过程 (summary, encrypted_content)
//!   - function_call: 函数调用 (name, call_id, arguments)
//!   - function_call_output: 函数输出 (call_id, output)
//!   - custom_tool_call: 自定义工具调用
//!   - custom_tool_call_output: 自定义工具输出
//!   - ghost_snapshot: 快照
//! - compacted: 上下文压缩摘要
//! - event_msg: 事件消息 (与 response_item 部分重复，以 response_item 为主)

use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::{AdapterMeta, ConversationAdapter};
use crate::domain::{MessageType, ParseResult, ParsedContent, ParsedMessage, SessionMeta, Source};

// ============================================================================
// 适配器元信息（静态配置）
// ============================================================================

/// Codex CLI 适配器元信息
pub static CODEX_META: AdapterMeta = AdapterMeta {
    source: Source::Codex,
    name: "Codex CLI",
    default_path: ".codex",
    env_var: "CODEX_PATH",
    extensions: &["jsonl"],
    recursive: true,
};

/// history.jsonl 条目
#[derive(Debug, Deserialize)]
struct HistoryEntry {
    session_id: String,
    ts: serde_json::Value, // 可能是数字或字符串
    text: Option<String>,
}

/// Rollout 事件（通用结构）
/// 格式: { timestamp, type, payload: {...} }
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct RolloutEvent {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    event_type: Option<String>,
    payload: Option<serde_json::Value>,
}

/// session_meta payload
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct SessionMetaPayload {
    id: Option<String>,
    cwd: Option<String>,
    cli_version: Option<String>,
    source: Option<String>,
    originator: Option<String>,
    instructions: Option<serde_json::Value>,
    git: Option<GitInfo>,
}

/// Git 信息
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct GitInfo {
    commit_hash: Option<String>,
    branch: Option<String>,
    repository_url: Option<String>,
}

/// turn_context payload
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct TurnContextPayload {
    cwd: Option<String>,
    model: Option<String>,
    approval_policy: Option<String>,
    sandbox_policy: Option<serde_json::Value>,
    summary: Option<String>,
}

/// response_item payload
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct ResponseItemPayload {
    #[serde(rename = "type")]
    item_type: Option<String>,
    role: Option<String>,
    content: Option<Vec<ContentBlock>>,
    id: Option<String>,
    // function_call 字段
    name: Option<String>,
    call_id: Option<String>,
    arguments: Option<String>,
    // function_call_output 字段
    output: Option<String>,
    // reasoning 字段
    summary: Option<Vec<SummaryBlock>>,
    encrypted_content: Option<String>,
}

/// 内容块
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: Option<String>,
    text: Option<String>,
    // tool 相关
    name: Option<String>,
    call_id: Option<String>,
    arguments: Option<String>,
    output: Option<String>,
}

/// reasoning summary 块
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct SummaryBlock {
    #[serde(rename = "type")]
    block_type: Option<String>,
    text: Option<String>,
}

/// compacted payload
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct CompactedPayload {
    message: Option<String>,
}

/// Codex CLI 适配器
pub struct CodexAdapter {
    /// Codex 根目录 (~/.codex)
    data_path: PathBuf,
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexAdapter {
    /// 创建适配器（使用默认路径或环境变量）
    pub fn new() -> Self {
        let path = std::env::var(CODEX_META.env_var)
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_default()
                    .join(CODEX_META.default_path)
            });
        Self { data_path: path }
    }

    /// 使用自定义路径创建适配器
    pub fn with_path(path: PathBuf) -> Self {
        Self { data_path: path }
    }

    /// history.jsonl 路径
    fn history_path(&self) -> PathBuf {
        self.data_path.join("history.jsonl")
    }

    /// sessions 根目录
    fn sessions_root(&self) -> PathBuf {
        self.data_path.join("sessions")
    }

    /// 解析时间戳（秒或毫秒）
    fn parse_timestamp(ts: &serde_json::Value) -> Option<i64> {
        let num = match ts {
            serde_json::Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
            serde_json::Value::String(s) => s.parse::<i64>().ok(),
            _ => None,
        }?;
        // 判断是秒还是毫秒
        if num > 1_000_000_000_000 {
            Some(num) // 已经是毫秒
        } else {
            Some(num * 1000) // 秒转毫秒
        }
    }

    /// 格式化时间戳为字符串
    fn format_timestamp(ts: &serde_json::Value) -> Option<String> {
        Self::parse_timestamp(ts).map(|ms| ms.to_string())
    }

    /// 从时间戳获取日期目录路径
    fn get_date_dir(&self, ts_ms: i64) -> PathBuf {
        use chrono::{TimeZone, Utc};
        let dt = Utc
            .timestamp_millis_opt(ts_ms)
            .single()
            .unwrap_or_else(Utc::now);
        let year = dt.format("%Y").to_string();
        let month = dt.format("%m").to_string();
        let day = dt.format("%d").to_string();
        self.sessions_root().join(year).join(month).join(day)
    }

    /// 解析 session 路径
    /// 优先在对应日期目录查找，找不到则递归搜索
    fn resolve_session_path(&self, session_id: &str, ts: Option<i64>) -> Option<PathBuf> {
        // 优先在日期目录查找
        if let Some(ts_ms) = ts {
            let day_dir = self.get_date_dir(ts_ms);
            if day_dir.exists()
                && let Ok(entries) = fs::read_dir(&day_dir)
            {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.contains(session_id) {
                        return Some(entry.path());
                    }
                }
            }
        }

        // 递归搜索
        self.search_session_recursive(&self.sessions_root(), session_id)
    }

    /// 递归搜索 session 文件
    fn search_session_recursive(&self, root: &Path, session_id: &str) -> Option<PathBuf> {
        let mut stack = vec![root.to_path_buf()];

        while let Some(dir) = stack.pop() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.contains(session_id) {
                        return Some(entry.path());
                    }
                    // 可能是目录（年/月/日）
                    if entry.path().is_dir() {
                        stack.push(entry.path());
                    }
                }
            }
        }
        None
    }

    /// 从 rollout 文件提取 cwd
    /// 格式: { type: "session_meta", payload: { cwd: "..." } }
    ///    或: { type: "turn_context", payload: { cwd: "..." } }
    fn extract_cwd_from_rollout(path: &Path) -> Option<String> {
        let file = fs::File::open(path).ok()?;
        let reader = BufReader::new(file);

        for line in reader.lines().take(10) {
            let line = line.ok()?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<RolloutEvent>(&line) {
                let event_type = event.event_type.as_deref();

                // session_meta 或 turn_context 都可能有 cwd
                if matches!(event_type, Some("session_meta") | Some("turn_context"))
                    && let Some(payload) = &event.payload
                    && let Some(cwd) = payload.get("cwd").and_then(|v| v.as_str())
                {
                    return Some(cwd.to_string());
                }
            }
        }
        None
    }

    /// 从路径提取项目名（跨平台）
    fn extract_project_name(path: &str) -> String {
        std::path::Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path)
            .to_string()
    }

    /// 从 payload 提取 content 文本
    /// 格式: { content: [{ type: "input_text"|"text", text: "..." }, ...] }
    fn extract_content_text(payload: &serde_json::Value) -> String {
        let mut texts = Vec::new();

        if let Some(content_arr) = payload.get("content").and_then(|v| v.as_array()) {
            for block in content_arr {
                let block_type = block.get("type").and_then(|v| v.as_str());

                match block_type {
                    Some("input_text") | Some("text") | Some("output_text") => {
                        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                            texts.push(text.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }

        texts.join("\n")
    }

    /// 解析 rollout 文件（全量采集）
    ///
    /// 采集所有事件类型：
    /// - session_meta: 元数据 → 存入 session_meta_list
    /// - turn_context: 上下文 → 存入 turn_contexts
    /// - response_item (message): 消息 → ParsedMessage
    /// - response_item (reasoning): 推理 → ParsedMessage (meta 存 summary/encrypted)
    /// - response_item (function_call): 工具调用 → ParsedMessage
    /// - response_item (function_call_output): 工具输出 → ParsedMessage (Tool)
    /// - response_item (custom_tool_call): 自定义工具 → ParsedMessage
    /// - response_item (custom_tool_call_output): 自定义工具输出 → ParsedMessage (Tool)
    /// - response_item (ghost_snapshot): 快照 → ParsedMessage (meta 存 payload)
    /// - compacted: 压缩摘要 → ParsedMessage (System)
    fn parse_rollout(&self, meta: &SessionMeta) -> Result<ParseResult> {
        let session_path = meta
            .session_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("缺少 session_path"))?;

        let file = fs::File::open(session_path)?;
        let reader = BufReader::new(file);

        let mut messages = Vec::new();
        let mut cwd = meta.cwd.clone();
        let mut model = meta.model.clone();
        let mut first_timestamp: Option<String> = None;
        let mut last_timestamp: Option<String> = None;
        let mut sequence = 0i64;

        // 额外元数据收集
        let mut session_metas: Vec<serde_json::Value> = Vec::new();
        let mut turn_contexts: Vec<serde_json::Value> = Vec::new();
        let mut git_info: Option<serde_json::Value> = None;
        let mut cli_version: Option<String> = None;

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let Ok(event) = serde_json::from_str::<RolloutEvent>(&line) else {
                continue;
            };

            let event_type = event.event_type.as_deref();
            let timestamp = event.timestamp.clone();

            // 更新时间戳
            if first_timestamp.is_none() {
                first_timestamp = timestamp.clone();
            }
            last_timestamp = timestamp.clone();

            match event_type {
                Some("session_meta") => {
                    // 全量采集 session_meta
                    if let Some(payload) = &event.payload {
                        session_metas.push(payload.clone());

                        if cwd.is_none() {
                            cwd = payload
                                .get("cwd")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        if cli_version.is_none() {
                            cli_version = payload
                                .get("cli_version")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        if git_info.is_none() {
                            git_info = payload.get("git").cloned();
                        }
                    }
                }
                Some("turn_context") => {
                    // 全量采集 turn_context
                    if let Some(payload) = &event.payload {
                        turn_contexts.push(payload.clone());

                        if cwd.is_none() {
                            cwd = payload
                                .get("cwd")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                        if model.is_none() {
                            model = payload
                                .get("model")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                        }
                    }
                }
                Some("response_item") => {
                    if let Some(payload) = &event.payload {
                        let item_type = payload.get("type").and_then(|v| v.as_str());

                        match item_type {
                            Some("message") => {
                                // 用户/助手消息
                                let role = payload.get("role").and_then(|v| v.as_str());
                                let content_text = Self::extract_content_text(payload);

                                // 跳过空消息，但保留环境上下文（放入 meta）
                                let is_env_context = content_text.contains("<environment_context>");

                                let msg_type = match role {
                                    Some("user") => MessageType::User,
                                    Some("assistant") => MessageType::Assistant,
                                    _ => MessageType::User,
                                };

                                let msg_id = payload
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .map(String::from)
                                    .unwrap_or_else(|| {
                                        format!(
                                            "{}:{}:{}",
                                            meta.id,
                                            role.unwrap_or("msg"),
                                            sequence
                                        )
                                    });

                                // 环境上下文消息：text 为空（不向量化），full 保留完整内容
                                let (text, full) = if is_env_context {
                                    (String::new(), content_text.clone())
                                } else {
                                    (content_text.clone(), content_text.clone())
                                };

                                if !full.is_empty() || is_env_context {
                                    messages.push(ParsedMessage {
                                        uuid: msg_id,
                                        session_id: meta.id.clone(),
                                        message_type: msg_type,
                                        content: ParsedContent { text, full },
                                        timestamp: timestamp.clone(),
                                        source: Source::Codex,
                                        channel: Some("cli".to_string()),
                                        model: model.clone(),
                                        tool_call_id: None,
                                        tool_name: None,
                                        tool_args: None,
                                        raw: Some(line.clone()),
                                        cwd: cwd.clone(),
                                        stop_reason: None,
                                    });
                                    sequence += 1;
                                }
                            }
                            Some("reasoning") => {
                                // 推理过程 - 提取 summary 文本，encrypted_content 存 meta
                                let summary_text = payload
                                    .get("summary")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    })
                                    .unwrap_or_default();

                                // encrypted_content 已存入 raw 字段，无需额外处理
                                messages.push(ParsedMessage {
                                    uuid: format!("{}:reasoning:{}", meta.id, sequence),
                                    session_id: meta.id.clone(),
                                    message_type: MessageType::Assistant,
                                    content: ParsedContent {
                                        text: String::new(), // reasoning 不做向量化
                                        full: format!("[Reasoning]\n{}", summary_text),
                                    },
                                    timestamp: timestamp.clone(),
                                    source: Source::Codex,
                                    channel: Some("cli".to_string()),
                                    model: model.clone(),
                                    tool_call_id: None,
                                    tool_name: Some("reasoning".to_string()),
                                    tool_args: None,
                                    raw: Some(line.clone()),
                                    cwd: cwd.clone(),
                                    stop_reason: None,
                                });
                                sequence += 1;
                            }
                            Some("function_call") => {
                                // 标准函数调用
                                let name = payload.get("name").and_then(|v| v.as_str());
                                let call_id = payload.get("call_id").and_then(|v| v.as_str());
                                let arguments = payload.get("arguments").and_then(|v| v.as_str());

                                messages.push(ParsedMessage {
                                    uuid: call_id.map(String::from).unwrap_or_else(|| {
                                        format!("{}:func:{}", meta.id, sequence)
                                    }),
                                    session_id: meta.id.clone(),
                                    message_type: MessageType::Assistant,
                                    content: ParsedContent {
                                        text: String::new(),
                                        full: format!("[Function: {}]", name.unwrap_or("unknown")),
                                    },
                                    timestamp: timestamp.clone(),
                                    source: Source::Codex,
                                    channel: Some("cli".to_string()),
                                    model: model.clone(),
                                    tool_call_id: call_id.map(String::from),
                                    tool_name: name.map(String::from),
                                    tool_args: arguments.map(String::from),
                                    raw: Some(line.clone()),
                                    cwd: cwd.clone(),
                                    stop_reason: None,
                                });
                                sequence += 1;
                            }
                            Some("function_call_output") => {
                                // 函数输出
                                let output =
                                    payload.get("output").and_then(|v| v.as_str()).unwrap_or("");
                                let call_id = payload.get("call_id").and_then(|v| v.as_str());

                                messages.push(ParsedMessage {
                                    uuid: format!("{}:func_out:{}", meta.id, sequence),
                                    session_id: meta.id.clone(),
                                    message_type: MessageType::Tool,
                                    content: ParsedContent {
                                        text: String::new(),
                                        full: output.to_string(),
                                    },
                                    timestamp: timestamp.clone(),
                                    source: Source::Codex,
                                    channel: Some("cli".to_string()),
                                    model: None,
                                    tool_call_id: call_id.map(String::from),
                                    tool_name: None,
                                    tool_args: None,
                                    raw: Some(line.clone()),
                                    cwd: cwd.clone(),
                                    stop_reason: None,
                                });
                                sequence += 1;
                            }
                            Some("custom_tool_call") => {
                                // 自定义工具调用
                                let name = payload.get("name").and_then(|v| v.as_str());
                                let call_id = payload.get("id").and_then(|v| v.as_str());
                                let arguments = payload.get("arguments").map(|v| v.to_string());

                                messages.push(ParsedMessage {
                                    uuid: call_id.map(String::from).unwrap_or_else(|| {
                                        format!("{}:custom:{}", meta.id, sequence)
                                    }),
                                    session_id: meta.id.clone(),
                                    message_type: MessageType::Assistant,
                                    content: ParsedContent {
                                        text: String::new(),
                                        full: format!(
                                            "[CustomTool: {}]",
                                            name.unwrap_or("unknown")
                                        ),
                                    },
                                    timestamp: timestamp.clone(),
                                    source: Source::Codex,
                                    channel: Some("cli".to_string()),
                                    model: model.clone(),
                                    tool_call_id: call_id.map(String::from),
                                    tool_name: name.map(String::from),
                                    tool_args: arguments,
                                    raw: Some(line.clone()),
                                    cwd: cwd.clone(),
                                    stop_reason: None,
                                });
                                sequence += 1;
                            }
                            Some("custom_tool_call_output") => {
                                // 自定义工具输出
                                let output = payload
                                    .get("output")
                                    .map(|v| {
                                        if v.is_string() {
                                            v.as_str().unwrap_or("").to_string()
                                        } else {
                                            v.to_string()
                                        }
                                    })
                                    .unwrap_or_default();
                                let call_id = payload.get("id").and_then(|v| v.as_str());

                                messages.push(ParsedMessage {
                                    uuid: format!("{}:custom_out:{}", meta.id, sequence),
                                    session_id: meta.id.clone(),
                                    message_type: MessageType::Tool,
                                    content: ParsedContent {
                                        text: String::new(),
                                        full: output,
                                    },
                                    timestamp: timestamp.clone(),
                                    source: Source::Codex,
                                    channel: Some("cli".to_string()),
                                    model: None,
                                    tool_call_id: call_id.map(String::from),
                                    tool_name: None,
                                    tool_args: None,
                                    raw: Some(line.clone()),
                                    cwd: cwd.clone(),
                                    stop_reason: None,
                                });
                                sequence += 1;
                            }
                            Some("ghost_snapshot") => {
                                // 快照 - 存入 meta
                                messages.push(ParsedMessage {
                                    uuid: format!("{}:snapshot:{}", meta.id, sequence),
                                    session_id: meta.id.clone(),
                                    message_type: MessageType::Assistant,
                                    content: ParsedContent {
                                        text: String::new(),
                                        full: "[GhostSnapshot]".to_string(),
                                    },
                                    timestamp: timestamp.clone(),
                                    source: Source::Codex,
                                    channel: Some("cli".to_string()),
                                    model: model.clone(),
                                    tool_call_id: None,
                                    tool_name: Some("ghost_snapshot".to_string()),
                                    tool_args: None,
                                    raw: Some(line.clone()),
                                    cwd: cwd.clone(),
                                    stop_reason: None,
                                });
                                sequence += 1;
                            }
                            _ => {
                                // 未知类型 - 仍然保存，存入 raw
                                if let Some(item_type_str) = item_type {
                                    tracing::debug!("未知 response_item 类型: {}", item_type_str);
                                    messages.push(ParsedMessage {
                                        uuid: format!("{}:unknown:{}", meta.id, sequence),
                                        session_id: meta.id.clone(),
                                        message_type: MessageType::Assistant,
                                        content: ParsedContent {
                                            text: String::new(),
                                            full: format!("[Unknown: {}]", item_type_str),
                                        },
                                        timestamp: timestamp.clone(),
                                        source: Source::Codex,
                                        channel: Some("cli".to_string()),
                                        model: model.clone(),
                                        tool_call_id: None,
                                        tool_name: Some(item_type_str.to_string()),
                                        tool_args: None,
                                        raw: Some(line.clone()),
                                        cwd: cwd.clone(),
                                        stop_reason: None,
                                    });
                                    sequence += 1;
                                }
                            }
                        }
                    }
                }
                Some("compacted") => {
                    // 上下文压缩摘要 - 重要信息，作为系统消息保存
                    if let Some(payload) = &event.payload {
                        let message = payload
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        if !message.is_empty() {
                            messages.push(ParsedMessage {
                                uuid: format!("{}:compacted:{}", meta.id, sequence),
                                session_id: meta.id.clone(),
                                message_type: MessageType::Assistant,
                                content: ParsedContent {
                                    text: String::new(), // compacted 不做向量化
                                    full: format!("[Compacted Summary]\n{}", message),
                                },
                                timestamp: timestamp.clone(),
                                source: Source::Codex,
                                channel: Some("cli".to_string()),
                                model: model.clone(),
                                tool_call_id: None,
                                tool_name: Some("compacted".to_string()),
                                tool_args: None,
                                raw: Some(line.clone()),
                                cwd: cwd.clone(),
                                stop_reason: None,
                            });
                            sequence += 1;
                        }
                    }
                }
                // event_msg 不单独处理，因为 response_item 已经包含完整信息
                Some("event_msg") => {
                    // 跳过，避免重复
                }
                _ => {
                    // 其他未知事件类型，记录日志
                    if let Some(et) = event_type {
                        tracing::debug!("跳过未知事件类型: {}", et);
                    }
                }
            }
        }

        // 构建会话级元数据
        let result_meta = serde_json::json!({
            "cli_version": cli_version,
            "git": git_info,
            "session_meta_count": session_metas.len(),
            "turn_context_count": turn_contexts.len(),
        });

        Ok(ParseResult {
            messages,
            created_at: first_timestamp,
            updated_at: last_timestamp,
            cwd,
            model,
            meta: Some(result_meta),
        })
    }
}

impl ConversationAdapter for CodexAdapter {
    fn meta(&self) -> &'static AdapterMeta {
        &CODEX_META
    }

    fn data_path(&self) -> &Path {
        &self.data_path
    }

    fn list_sessions(&self) -> Result<Vec<SessionMeta>> {
        let mut results = Vec::new();
        let history_path = self.history_path();

        if !history_path.exists() {
            tracing::warn!("Codex history.jsonl 不存在: {:?}", history_path);
            return Ok(results);
        }

        let file = fs::File::open(&history_path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let Ok(entry) = serde_json::from_str::<HistoryEntry>(&line) else {
                continue;
            };

            let ts_ms = Self::parse_timestamp(&entry.ts);
            let session_path = self.resolve_session_path(&entry.session_id, ts_ms);

            // 从 rollout 文件提取 cwd
            let cwd = session_path
                .as_ref()
                .and_then(|p| Self::extract_cwd_from_rollout(p));

            let project_path = cwd
                .clone()
                .unwrap_or_else(|| self.sessions_root().to_string_lossy().to_string());
            let project_name = Self::extract_project_name(&project_path);

            // 获取文件元数据
            let (file_mtime, file_size) = session_path
                .as_ref()
                .and_then(|p| fs::metadata(p).ok())
                .map(|m| {
                    let mtime = m
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64);
                    (mtime, Some(m.len()))
                })
                .unwrap_or((None, None));

            let timestamp = Self::format_timestamp(&entry.ts);

            results.push(SessionMeta {
                id: entry.session_id.clone(),
                source: Source::Codex,
                channel: Some("cli".to_string()),
                project_path,
                project_name: Some(project_name),
                encoded_dir_name: None,
                session_path: session_path.map(|p| p.to_string_lossy().to_string()),
                file_mtime,
                file_size,
                message_count: None,
                cwd,
                model: None,
                meta: entry.text.map(|t| {
                    serde_json::json!({
                        "historyText": t,
                        "historyTs": entry.ts
                    })
                }),
                created_at: timestamp.clone(),
                updated_at: timestamp,
            });
        }

        Ok(results)
    }

    fn parse_session(&self, meta: &SessionMeta) -> Result<Option<ParseResult>> {
        if meta.session_path.is_none() {
            tracing::debug!("Codex 会话缺少 session_path: {}", meta.id);
            return Ok(None);
        }

        let session_path = Path::new(meta.session_path.as_ref().unwrap());
        if !session_path.exists() {
            tracing::debug!("Codex session 文件不存在: {:?}", session_path);
            return Ok(None);
        }

        let result = self.parse_rollout(meta)?;
        Ok(Some(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timestamp_millis() {
        let ts = serde_json::json!(1768452489295i64);
        assert_eq!(CodexAdapter::parse_timestamp(&ts), Some(1768452489295));
    }

    #[test]
    fn test_parse_timestamp_seconds() {
        let ts = serde_json::json!(1768452489);
        assert_eq!(CodexAdapter::parse_timestamp(&ts), Some(1768452489000));
    }

    #[test]
    fn test_parse_timestamp_string() {
        let ts = serde_json::json!("1768452489295");
        assert_eq!(CodexAdapter::parse_timestamp(&ts), Some(1768452489295));
    }
}
