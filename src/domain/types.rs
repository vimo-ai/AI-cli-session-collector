//! 领域类型定义

use serde::{Deserialize, Serialize};

/// 数据来源标识
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Claude,
    Codex,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Claude => write!(f, "claude"),
            Source::Codex => write!(f, "codex"),
        }
    }
}

/// 消息类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    User,
    Assistant,
    Tool,
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageType::User => write!(f, "user"),
            MessageType::Assistant => write!(f, "assistant"),
            MessageType::Tool => write!(f, "tool"),
        }
    }
}

impl std::str::FromStr for MessageType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "human" | "user" => Ok(MessageType::User),
            "assistant" => Ok(MessageType::Assistant),
            "tool" => Ok(MessageType::Tool),
            _ => Err(format!("Unknown message type: {}", s)),
        }
    }
}

/// 标准化的会话元数据
#[derive(Debug, Clone, Serialize)]
pub struct SessionMeta {
    /// 会话 ID
    pub id: String,
    /// 数据来源
    pub source: Source,
    /// 渠道 (cli/code/gui)
    pub channel: Option<String>,
    /// 关联项目真实路径
    pub project_path: String,
    /// 项目名称
    pub project_name: Option<String>,
    /// Claude 编码目录名
    pub encoded_dir_name: Option<String>,
    /// 会话文件完整路径
    pub session_path: Option<String>,
    /// 文件修改时间戳 (毫秒)
    pub file_mtime: Option<u64>,
    /// 文件大小
    pub file_size: Option<u64>,
    /// 消息数量 (JSONL 行数)
    #[serde(rename = "messageCount")]
    pub message_count: Option<usize>,
    /// 工作目录
    pub cwd: Option<String>,
    /// 默认模型
    pub model: Option<String>,
    /// 额外元信息
    pub meta: Option<serde_json::Value>,
    /// 创建时间
    pub created_at: Option<String>,
    /// 更新时间
    pub updated_at: Option<String>,
}

/// 解析后的内容（分离 text 和 full）
#[derive(Debug, Clone, Serialize, Default)]
pub struct ParsedContent {
    /// 纯对话文本（用于向量化）
    pub text: String,
    /// 完整格式化内容（用于 FTS，包含 tool_use 等）
    pub full: String,
}

/// 标准化的消息
#[derive(Debug, Clone, Serialize)]
pub struct ParsedMessage {
    /// 消息 UUID
    pub uuid: String,
    /// 会话 ID
    pub session_id: String,
    /// 消息类型
    pub message_type: MessageType,
    /// 消息内容（分离版本）
    pub content: ParsedContent,
    /// 时间戳
    pub timestamp: Option<String>,
    /// 数据来源
    pub source: Source,
    /// 渠道
    pub channel: Option<String>,
    /// 模型
    pub model: Option<String>,
    /// Tool call ID
    pub tool_call_id: Option<String>,
    /// Tool 名称
    pub tool_name: Option<String>,
    /// Tool 参数
    pub tool_args: Option<String>,
    /// 原始数据
    pub raw: Option<String>,
}

/// 适配器解析结果
#[derive(Debug, Clone)]
pub struct ParseResult {
    pub messages: Vec<ParsedMessage>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub meta: Option<serde_json::Value>,
}

/// 可索引的会话数据（用于写入数据库）
/// 包含正确解析的项目路径（从 cwd 读取，解决中文路径问题）
#[derive(Debug, Clone, Serialize)]
pub struct IndexableSession {
    /// 会话 ID
    pub session_id: String,
    /// 项目真实路径（从 JSONL 的 cwd 字段读取）
    pub project_path: String,
    /// 项目名称
    pub project_name: String,
    /// 消息列表
    pub messages: Vec<IndexableMessage>,
}

/// 可索引的消息
#[derive(Debug, Clone, Serialize)]
pub struct IndexableMessage {
    /// 消息 UUID
    pub uuid: String,
    /// 角色 (user/assistant)
    pub role: String,
    /// 消息内容（分离版本）
    pub content: ParsedContent,
    /// 时间戳 (毫秒)
    pub timestamp: i64,
    /// 序号
    pub sequence: i64,
    /// 原始 JSONL 行
    pub raw: Option<String>,
}
