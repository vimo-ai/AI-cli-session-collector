//! L1: 按 type 检查字段组合规则

use crate::validate::framework::*;

use super::super::framework::truncate_with_ellipsis;

/// L1 验证：检查每种 type 的必选/条件字段组合
pub fn validate_line(
    line_number: usize,
    entry_type: &str,
    obj: &serde_json::Map<String, serde_json::Value>,
    session_id: &str,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    match entry_type {
        "user" => {
            // user 消息必须有 message
            if !obj.contains_key("message") {
                // 如果有 toolUseResult 则是 tool_result 伪装的 user，允许无 message
                if !obj.contains_key("toolUseResult") {
                    findings.push(Finding {
                        level: Level::L1Line,
                        severity: Severity::Warning,
                        rule_id: "L1-USER-NO-MESSAGE".to_string(),
                        message: "user 行缺少 message 字段".to_string(),
                        session_id: Some(session_id.to_string()),
                        line_number: Some(line_number),
                        context: None,
                    });
                }
            }
            // user 消息通常有 cwd（首条必须有）
            // 不强制报错，L4 级别再检查
        }
        "assistant" => {
            // assistant 必须有 message
            if !obj.contains_key("message") {
                findings.push(Finding {
                    level: Level::L1Line,
                    severity: Severity::Error,
                    rule_id: "L1-ASSISTANT-NO-MESSAGE".to_string(),
                    message: "assistant 行缺少 message 字段".to_string(),
                    session_id: Some(session_id.to_string()),
                    line_number: Some(line_number),
                    context: None,
                });
            }
            // assistant 通常有 requestId
            if !obj.contains_key("requestId") {
                findings.push(Finding {
                    level: Level::L1Line,
                    severity: Severity::Warning,
                    rule_id: "L1-ASSISTANT-NO-REQUEST-ID".to_string(),
                    message: "assistant 行缺少 requestId".to_string(),
                    session_id: Some(session_id.to_string()),
                    line_number: Some(line_number),
                    context: None,
                });
            }
        }
        "system" => {
            // system 必须有 subtype
            if !obj.contains_key("subtype") {
                findings.push(Finding {
                    level: Level::L1Line,
                    severity: Severity::Warning,
                    rule_id: "L1-SYSTEM-NO-SUBTYPE".to_string(),
                    message: "system 行缺少 subtype 字段".to_string(),
                    session_id: Some(session_id.to_string()),
                    line_number: Some(line_number),
                    context: None,
                });
            }
            // compact_boundary 必须有 logicalParentUuid 和 compactMetadata
            if let Some(subtype) = obj.get("subtype").and_then(|v| v.as_str()) {
                match subtype {
                    "compact_boundary" | "microcompact_boundary" => {
                        if !obj.contains_key("logicalParentUuid") {
                            findings.push(Finding {
                                level: Level::L1Line,
                                severity: Severity::Warning,
                                rule_id: "L1-COMPACT-NO-LOGICAL-PARENT".to_string(),
                                message: format!("{} 缺少 logicalParentUuid", subtype),
                                session_id: Some(session_id.to_string()),
                                line_number: Some(line_number),
                                context: None,
                            });
                        }
                    }
                    "turn_duration" => {
                        if !obj.contains_key("durationMs") && !obj.contains_key("data") {
                            findings.push(Finding {
                                level: Level::L1Line,
                                severity: Severity::Warning,
                                rule_id: "L1-TURN-DURATION-NO-DATA".to_string(),
                                message: "turn_duration 缺少 durationMs 或 data".to_string(),
                                session_id: Some(session_id.to_string()),
                                line_number: Some(line_number),
                                context: None,
                            });
                        }
                    }
                    "stop_hook_summary" => {
                        // 通常有 hookCount
                    }
                    _ => {} // 未知 subtype 在 L0 的值域检查中已经报告
                }
            }
        }
        "summary" => {
            if !obj.contains_key("summary") {
                findings.push(Finding {
                    level: Level::L1Line,
                    severity: Severity::Warning,
                    rule_id: "L1-SUMMARY-NO-CONTENT".to_string(),
                    message: "summary 行缺少 summary 字段".to_string(),
                    session_id: Some(session_id.to_string()),
                    line_number: Some(line_number),
                    context: None,
                });
            }
        }
        "custom-title" => {
            if !obj.contains_key("customTitle") {
                findings.push(Finding {
                    level: Level::L1Line,
                    severity: Severity::Warning,
                    rule_id: "L1-CUSTOM-TITLE-NO-CONTENT".to_string(),
                    message: "custom-title 行缺少 customTitle 字段".to_string(),
                    session_id: Some(session_id.to_string()),
                    line_number: Some(line_number),
                    context: None,
                });
            }
        }
        // progress, file-history-snapshot, queue-operation 无特殊字段要求
        _ => {}
    }

    // 通用：uuid 和 parentUuid 的配对检查
    // 有 parentUuid 但无 uuid 是异常的
    if obj.contains_key("parentUuid") && !obj.contains_key("uuid") {
        findings.push(Finding {
            level: Level::L1Line,
            severity: Severity::Warning,
            rule_id: "L1-PARENT-WITHOUT-UUID".to_string(),
            message: "有 parentUuid 但无 uuid".to_string(),
            session_id: Some(session_id.to_string()),
            line_number: Some(line_number),
            context: None,
        });
    }

    findings
}

/// 验证 message 子对象的 content blocks
pub fn validate_message_content_blocks(
    line_number: usize,
    message: &serde_json::Map<String, serde_json::Value>,
    session_id: &str,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    let content = match message.get("content") {
        Some(c) => c,
        None => return findings,
    };

    // content 是 array 才需要检查 blocks
    let blocks = match content.as_array() {
        Some(arr) => arr,
        None => return findings, // string content 不需要检查
    };

    for (i, block) in blocks.iter().enumerate() {
        let block_obj = match block.as_object() {
            Some(o) => o,
            None => {
                findings.push(Finding {
                    level: Level::L0Field,
                    severity: Severity::Error,
                    rule_id: "L0-BLOCK-NOT-OBJECT".to_string(),
                    message: format!("content block[{}] 不是 object", i),
                    session_id: Some(session_id.to_string()),
                    line_number: Some(line_number),
                    context: Some(format!("{}", block)),
                });
                continue;
            }
        };

        let block_type = block_obj
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        if let Some(whitelist) =
            super::schema::block_whitelist_for_type(block_type)
        {
            // 检查未知字段
            let unknown =
                super::schema::check_field_whitelist(block_obj, whitelist);
            for field_name in &unknown {
                findings.push(Finding {
                    level: Level::L0Field,
                    severity: Severity::Warning,
                    rule_id: "L0-UNKNOWN-BLOCK-FIELD".to_string(),
                    message: format!(
                        "content block[{}] (type={}) 未知字段: {}",
                        i, block_type, field_name
                    ),
                    session_id: Some(session_id.to_string()),
                    line_number: Some(line_number),
                    context: block_obj
                        .get(field_name)
                        .map(|v| {
                            let s = v.to_string();
                            truncate_with_ellipsis(&s, 60)
                        }),
                });
            }

            // 检查类型
            let mismatches =
                super::schema::check_field_types(block_obj, whitelist);
            for (fname, expected, actual) in &mismatches {
                findings.push(Finding {
                    level: Level::L0Field,
                    severity: Severity::Error,
                    rule_id: "L0-BLOCK-TYPE-MISMATCH".to_string(),
                    message: format!(
                        "block[{}].{}: 期望 {}, 实际 {}",
                        i, fname, expected, actual
                    ),
                    session_id: Some(session_id.to_string()),
                    line_number: Some(line_number),
                    context: None,
                });
            }
        } else {
            // 未知 block type
            findings.push(Finding {
                level: Level::L0Field,
                severity: Severity::Warning,
                rule_id: "L0-UNKNOWN-BLOCK-TYPE".to_string(),
                message: format!("未知 content block type: \"{}\"", block_type),
                session_id: Some(session_id.to_string()),
                line_number: Some(line_number),
                context: Some({
                    let s = block.to_string();
                    truncate_with_ellipsis(&s, 80)
                }),
            });
        }
    }

    findings
}
