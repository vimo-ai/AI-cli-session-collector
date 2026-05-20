//! L0: Claude Code JSONL 字段白名单
//!
//! 权威参考: docs/design/session-chain-linking.md

use crate::validate::framework::*;

use super::super::framework::truncate;

// ============================================================================
// 顶层字段白名单（~40 个）
// ============================================================================

pub const CLAUDE_TOP_LEVEL_FIELDS: &[FieldDescriptor] = &[
    // --- 核心标识 ---
    FieldDescriptor {
        name: "type",
        json_type: JsonType::String,
        presence: FieldPresence::Required,
        value_domain: Some(ValueDomain::Enum(&[
            "user",
            "assistant",
            "system",
            "summary",
            "custom-title",
            "progress",
            "file-history-snapshot",
            "queue-operation",
        ])),
    },
    FieldDescriptor {
        name: "uuid",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: Some(ValueDomain::UuidFormat),
    },
    FieldDescriptor {
        name: "parentUuid",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: Some(ValueDomain::UuidFormat),
    },
    FieldDescriptor {
        name: "timestamp",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None, // ISO8601 或毫秒字符串
    },
    FieldDescriptor {
        name: "sessionId",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    // --- 消息内容 ---
    FieldDescriptor {
        name: "message",
        json_type: JsonType::Object,
        presence: FieldPresence::Conditional,
        value_domain: None,
    },
    // --- 上下文字段 ---
    FieldDescriptor {
        name: "cwd",
        json_type: JsonType::String,
        presence: FieldPresence::Conditional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "gitBranch",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "isSidechain",
        json_type: JsonType::Bool,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "permissionMode",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "slug",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "version",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    // --- AI 响应字段 ---
    FieldDescriptor {
        name: "requestId",
        json_type: JsonType::String,
        presence: FieldPresence::Conditional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "stopReason",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: Some(ValueDomain::Enum(&[""])), // 始终为空字符串
    },
    FieldDescriptor {
        name: "durationMs",
        json_type: JsonType::Number,
        presence: FieldPresence::Optional,
        value_domain: Some(ValueDomain::NonNegative),
    },
    FieldDescriptor {
        name: "thinkingMetadata",
        json_type: JsonType::Object,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "model",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    // --- 压缩相关 ---
    FieldDescriptor {
        name: "isCompactSummary",
        json_type: JsonType::Bool,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "isVisibleInTranscriptOnly",
        json_type: JsonType::Bool,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "logicalParentUuid",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: Some(ValueDomain::UuidFormat),
    },
    FieldDescriptor {
        name: "subtype",
        json_type: JsonType::String,
        presence: FieldPresence::Conditional,
        value_domain: Some(ValueDomain::Enum(&[
            "stop_hook_summary",
            "turn_duration",
            "compact_boundary",
            "api_error",
            "local_command",
            "microcompact_boundary",
        ])),
    },
    FieldDescriptor {
        name: "compactMetadata",
        json_type: JsonType::Object,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    // --- 工具/Subagent ---
    FieldDescriptor {
        name: "toolUseID",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "toolUseResult",
        json_type: JsonType::Any,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "sourceToolUseID",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "sourceToolAssistantUUID",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "parentToolUseID",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    // --- 其他 ---
    FieldDescriptor {
        name: "isMeta",
        json_type: JsonType::Bool,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "userType",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "todos",
        json_type: JsonType::Array,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "hookCount",
        json_type: JsonType::Number,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "hookErrors",
        json_type: JsonType::Array,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "hookInfos",
        json_type: JsonType::Array,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "preventedContinuation",
        json_type: JsonType::Bool,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "hasOutput",
        json_type: JsonType::Bool,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "data",
        json_type: JsonType::Any,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "snapshot",
        json_type: JsonType::Any,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "isSnapshotUpdate",
        json_type: JsonType::Bool,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "summary",
        json_type: JsonType::String,
        presence: FieldPresence::Conditional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "customTitle",
        json_type: JsonType::String,
        presence: FieldPresence::Conditional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "content",
        json_type: JsonType::Any,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "level",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    // --- file-history-snapshot 专有 ---
    FieldDescriptor {
        name: "messageId",
        json_type: JsonType::String,
        presence: FieldPresence::Conditional,
        value_domain: Some(ValueDomain::UuidFormat),
    },
    // --- summary 专有 ---
    FieldDescriptor {
        name: "leafUuid",
        json_type: JsonType::String,
        presence: FieldPresence::Conditional,
        value_domain: Some(ValueDomain::UuidFormat),
    },
    // --- queue-operation 专有 ---
    FieldDescriptor {
        name: "operation",
        json_type: JsonType::String,
        presence: FieldPresence::Conditional,
        value_domain: Some(ValueDomain::Enum(&[
            "enqueue", "dequeue", "remove", "popAll",
        ])),
    },
    // --- user 输入附加 ---
    FieldDescriptor {
        name: "imagePasteIds",
        json_type: JsonType::Array,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "planContent",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    // --- assistant 错误标记 ---
    FieldDescriptor {
        name: "isApiErrorMessage",
        json_type: JsonType::Bool,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "error",
        json_type: JsonType::Any,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    // --- api_error 重试 ---
    FieldDescriptor {
        name: "cause",
        json_type: JsonType::Object,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "retryAttempt",
        json_type: JsonType::Number,
        presence: FieldPresence::Optional,
        value_domain: Some(ValueDomain::NonNegative),
    },
    FieldDescriptor {
        name: "maxRetries",
        json_type: JsonType::Number,
        presence: FieldPresence::Optional,
        value_domain: Some(ValueDomain::NonNegative),
    },
    FieldDescriptor {
        name: "retryInMs",
        json_type: JsonType::Number,
        presence: FieldPresence::Optional,
        value_domain: Some(ValueDomain::NonNegative),
    },
    // --- 微压缩 ---
    FieldDescriptor {
        name: "microcompactMetadata",
        json_type: JsonType::Object,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
];

// ============================================================================
// message 子字段白名单
// ============================================================================

pub const MESSAGE_FIELDS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "id",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "role",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: Some(ValueDomain::Enum(&["user", "assistant"])),
    },
    FieldDescriptor {
        name: "content",
        json_type: JsonType::StringOrArray,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "stop_reason",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: Some(ValueDomain::Enum(&[])), // 始终 null，非 null 字符串触发 L0 告警
    },
    FieldDescriptor {
        name: "stop_sequence",
        json_type: JsonType::Any,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "usage",
        json_type: JsonType::Object,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "model",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "type",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "context_management",
        json_type: JsonType::Any,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "container",
        json_type: JsonType::Any,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
];

// ============================================================================
// Content block 字段白名单
// ============================================================================

pub const THINKING_BLOCK_FIELDS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "type",
        json_type: JsonType::String,
        presence: FieldPresence::Required,
        value_domain: None,
    },
    FieldDescriptor {
        name: "thinking",
        json_type: JsonType::String,
        presence: FieldPresence::Required,
        value_domain: None,
    },
    FieldDescriptor {
        name: "signature",
        json_type: JsonType::String,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
];

pub const TEXT_BLOCK_FIELDS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "type",
        json_type: JsonType::String,
        presence: FieldPresence::Required,
        value_domain: None,
    },
    FieldDescriptor {
        name: "text",
        json_type: JsonType::String,
        presence: FieldPresence::Required,
        value_domain: None,
    },
    FieldDescriptor {
        name: "citations",
        json_type: JsonType::Array,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
];

pub const TOOL_USE_BLOCK_FIELDS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "type",
        json_type: JsonType::String,
        presence: FieldPresence::Required,
        value_domain: None,
    },
    FieldDescriptor {
        name: "id",
        json_type: JsonType::String,
        presence: FieldPresence::Required,
        value_domain: None,
    },
    FieldDescriptor {
        name: "name",
        json_type: JsonType::String,
        presence: FieldPresence::Required,
        value_domain: None,
    },
    FieldDescriptor {
        name: "input",
        json_type: JsonType::Any,
        presence: FieldPresence::Required,
        value_domain: None,
    },
];

pub const TOOL_RESULT_BLOCK_FIELDS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "type",
        json_type: JsonType::String,
        presence: FieldPresence::Required,
        value_domain: None,
    },
    FieldDescriptor {
        name: "tool_use_id",
        json_type: JsonType::String,
        presence: FieldPresence::Required,
        value_domain: None,
    },
    FieldDescriptor {
        name: "content",
        json_type: JsonType::Any,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
    FieldDescriptor {
        name: "is_error",
        json_type: JsonType::Bool,
        presence: FieldPresence::Optional,
        value_domain: None,
    },
];

pub const IMAGE_BLOCK_FIELDS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "type",
        json_type: JsonType::String,
        presence: FieldPresence::Required,
        value_domain: None,
    },
    FieldDescriptor {
        name: "source",
        json_type: JsonType::Object,
        presence: FieldPresence::Required,
        value_domain: None,
    },
];

// ============================================================================
// 项目级白名单
// ============================================================================

/// session 附属目录内允许的子目录名
pub const ALLOWED_SESSION_SUBDIRS: &[&str] = &["subagents", "tool-results"];

/// 项目目录内允许的非 JSONL 文件
pub const ALLOWED_PROJECT_FILES: &[&str] = &["sessions-index.json", "CLAUDE.md"];

/// sessions-index.json 字段白名单
pub const INDEX_ENTRY_FIELDS: &[&str] = &[
    "sessionId",
    "fullPath",
    "fileMtime",
    "firstPrompt",
    "customTitle",
    "summary",
    "messageCount",
    "created",
    "modified",
    "gitBranch",
    "projectPath",
    "isSidechain",
];

// ============================================================================
// L0 验证函数
// ============================================================================

/// 检查字段白名单，返回未知字段名列表
pub fn check_field_whitelist(
    obj: &serde_json::Map<String, serde_json::Value>,
    whitelist: &[FieldDescriptor],
) -> Vec<String> {
    let known: std::collections::HashSet<&str> = whitelist.iter().map(|f| f.name).collect();
    obj.keys()
        .filter(|k| !known.contains(k.as_str()))
        .cloned()
        .collect()
}

/// 检查字段类型是否匹配白名单
pub fn check_field_types(
    obj: &serde_json::Map<String, serde_json::Value>,
    whitelist: &[FieldDescriptor],
) -> Vec<(String, String, String)> {
    // 返回: (字段名, 期望类型, 实际类型)
    let mut mismatches = Vec::new();
    for desc in whitelist {
        if let Some(value) = obj.get(desc.name) {
            if value.is_null() {
                continue; // null 总是允许的
            }
            if !desc.json_type.matches(value) {
                mismatches.push((
                    desc.name.to_string(),
                    desc.json_type.to_string(),
                    json_type_name(value),
                ));
            }
        }
    }
    mismatches
}

/// 检查值域
pub fn check_value_domains(
    obj: &serde_json::Map<String, serde_json::Value>,
    whitelist: &[FieldDescriptor],
) -> Vec<(String, String)> {
    // 返回: (字段名, 违反描述)
    let mut violations = Vec::new();
    for desc in whitelist {
        if let (Some(value), Some(domain)) = (obj.get(desc.name), &desc.value_domain) {
            if value.is_null() {
                continue;
            }
            if let Some(violation) = check_domain(value, domain) {
                violations.push((desc.name.to_string(), violation));
            }
        }
    }
    violations
}

fn check_domain(value: &serde_json::Value, domain: &ValueDomain) -> Option<String> {
    match domain {
        ValueDomain::Enum(allowed) => {
            if let Some(s) = value.as_str() {
                if !allowed.contains(&s) {
                    return Some(format!("值 \"{}\" 不在枚举列表中", s));
                }
            }
            None
        }
        ValueDomain::UuidFormat => {
            if let Some(s) = value.as_str() {
                // UUID v4 格式：8-4-4-4-12 hex
                if s.len() != 36 || s.chars().filter(|c| *c == '-').count() != 4 {
                    // 允许非标准但合理的 UUID（如较短的）
                    if !s.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
                        return Some(format!("不像 UUID: \"{}\"", truncate(s, 20)));
                    }
                }
            }
            None
        }
        ValueDomain::Iso8601 => {
            if let Some(s) = value.as_str() {
                if chrono::DateTime::parse_from_rfc3339(s).is_err() && s.parse::<i64>().is_err() {
                    return Some(format!("无法解析时间戳: \"{}\"", truncate(s, 30)));
                }
            }
            None
        }
        ValueDomain::NonNegative => {
            if let Some(n) = value.as_f64() {
                if n < 0.0 {
                    return Some(format!("负数: {}", n));
                }
            }
            None
        }
    }
}

fn json_type_name(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(_) => "bool".to_string(),
        serde_json::Value::Number(_) => "number".to_string(),
        serde_json::Value::String(_) => "string".to_string(),
        serde_json::Value::Array(_) => "array".to_string(),
        serde_json::Value::Object(_) => "object".to_string(),
    }
}

/// 获取 content block 对应的字段白名单
pub fn block_whitelist_for_type(block_type: &str) -> Option<&'static [FieldDescriptor]> {
    match block_type {
        "thinking" => Some(THINKING_BLOCK_FIELDS),
        "text" => Some(TEXT_BLOCK_FIELDS),
        "tool_use" => Some(TOOL_USE_BLOCK_FIELDS),
        "tool_result" => Some(TOOL_RESULT_BLOCK_FIELDS),
        "image" => Some(IMAGE_BLOCK_FIELDS),
        _ => None,
    }
}
