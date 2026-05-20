//! 验证框架核心类型

use serde::Serialize;
use std::collections::HashMap;
use std::fmt;

/// 验证层级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Level {
    L0Field,
    L1Line,
    L2Block,
    L3Turn,
    L4Session,
    L5Project,
}

impl Level {
    pub fn as_u8(&self) -> u8 {
        match self {
            Level::L0Field => 0,
            Level::L1Line => 1,
            Level::L2Block => 2,
            Level::L3Turn => 3,
            Level::L4Session => 4,
            Level::L5Project => 5,
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Level::L0Field => write!(f, "L0 Field"),
            Level::L1Line => write!(f, "L1 Line"),
            Level::L2Block => write!(f, "L2 Block"),
            Level::L3Turn => write!(f, "L3 Turn"),
            Level::L4Session => write!(f, "L4 Session"),
            Level::L5Project => write!(f, "L5 Project"),
        }
    }
}

/// 严重程度
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Info => write!(f, "INFO"),
            Severity::Warning => write!(f, "WARN"),
            Severity::Error => write!(f, "ERROR"),
        }
    }
}

/// 单条验证发现
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub level: Level,
    pub severity: Severity,
    pub rule_id: String,
    pub message: String,
    pub session_id: Option<String>,
    pub line_number: Option<usize>,
    pub context: Option<String>,
}

/// 项目统计
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProjectStats {
    pub project_count: usize,
    pub session_count: usize,
    pub subagent_count: usize,
    pub total_lines: usize,
    pub known_field_count: usize,
    pub unknown_fields: HashMap<String, UnknownFieldInfo>,
    pub type_counts: HashMap<String, usize>,
    pub request_group_count: usize,
    pub turn_count: usize,
    pub fork_count: usize,
    pub compaction_count: usize,
}

/// 未知字段信息
#[derive(Debug, Clone, Serialize)]
pub struct UnknownFieldInfo {
    pub count: usize,
    pub first_session: Option<String>,
    pub first_line: Option<usize>,
    pub sample_value: Option<String>,
}

/// 验证结果
#[derive(Debug, Clone, Serialize)]
pub struct ValidationResult {
    pub findings: Vec<Finding>,
    pub stats: ProjectStats,
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidationResult {
    pub fn new() -> Self {
        Self {
            findings: Vec::new(),
            stats: ProjectStats::default(),
        }
    }

    pub fn has_errors(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .count()
    }

    pub fn merge(&mut self, other: ValidationResult) {
        self.findings.extend(other.findings);
        // stats 不自动 merge，由调用方管理
    }
}

// ============================================================================
// JSON 类型描述
// ============================================================================

/// JSON 值类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonType {
    String,
    Number,
    Bool,
    Object,
    Array,
    /// string 或 array（如 message.content）
    StringOrArray,
    /// 任意类型
    Any,
}

impl JsonType {
    /// 检查 serde_json::Value 是否匹配此类型
    pub fn matches(&self, value: &serde_json::Value) -> bool {
        match self {
            JsonType::String => value.is_string(),
            JsonType::Number => value.is_number(),
            JsonType::Bool => value.is_boolean(),
            JsonType::Object => value.is_object(),
            JsonType::Array => value.is_array(),
            JsonType::StringOrArray => value.is_string() || value.is_array(),
            JsonType::Any => true,
        }
    }
}

impl fmt::Display for JsonType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonType::String => write!(f, "string"),
            JsonType::Number => write!(f, "number"),
            JsonType::Bool => write!(f, "bool"),
            JsonType::Object => write!(f, "object"),
            JsonType::Array => write!(f, "array"),
            JsonType::StringOrArray => write!(f, "string|array"),
            JsonType::Any => write!(f, "any"),
        }
    }
}

/// 字段存在性
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldPresence {
    /// 必须存在
    Required,
    /// 可选
    Optional,
    /// 条件存在（依赖其他字段）
    Conditional,
}

/// 值域约束
#[derive(Debug, Clone)]
pub enum ValueDomain {
    /// 枚举值
    Enum(&'static [&'static str]),
    /// UUID 格式
    UuidFormat,
    /// ISO 8601 时间戳
    Iso8601,
    /// 非负数
    NonNegative,
}

/// 字段描述符
#[derive(Debug, Clone)]
pub struct FieldDescriptor {
    pub name: &'static str,
    pub json_type: JsonType,
    pub presence: FieldPresence,
    pub value_domain: Option<ValueDomain>,
}

// ============================================================================
// 中间表示（层间传递）
// ============================================================================

/// L0+L1 → L2 的输入
#[derive(Debug, Clone)]
pub struct ValidatedLine {
    pub line_number: usize,
    pub entry_type: String,
    pub uuid: Option<String>,
    pub parent_uuid: Option<String>,
    pub request_id: Option<String>,
    pub session_id: Option<String>,
    pub value: serde_json::Value,
}

/// L2 → L3 的输入
#[derive(Debug, Clone)]
pub struct RequestGroup {
    pub request_id: String,
    pub lines: Vec<ValidatedLine>,
}

/// L3 → L4 的输入
#[derive(Debug, Clone)]
pub struct Turn {
    pub turn_index: usize,
    pub user_line: ValidatedLine,
    pub request_groups: Vec<RequestGroup>,
    pub tool_result_lines: Vec<ValidatedLine>,
    pub system_lines: Vec<ValidatedLine>,
}

// ============================================================================
// Unicode-safe utilities
// ============================================================================

/// Unicode-safe string truncation
///
/// Returns a substring containing at most `max_chars` Unicode characters.
/// Always truncates at char boundaries, never splits a UTF-8 sequence.
pub fn truncate(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Unicode-safe string truncation with ellipsis
///
/// Returns a String containing at most `max_chars` Unicode characters,
/// with "..." appended if truncation occurred.
pub fn truncate_with_ellipsis(s: &str, max_chars: usize) -> String {
    let truncated = truncate(s, max_chars);
    if truncated.len() < s.len() {
        format!("{}...", truncated)
    } else {
        s.to_string()
    }
}
