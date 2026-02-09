//! L2: requestId 分组验证

use crate::validate::framework::*;
use std::collections::HashMap;

use super::super::framework::truncate_with_ellipsis;

/// 按 requestId 分组
pub fn group_by_request_id(lines: &[ValidatedLine]) -> Vec<RequestGroup> {
    let mut groups: HashMap<String, Vec<ValidatedLine>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for line in lines {
        if let Some(rid) = &line.request_id {
            if !groups.contains_key(rid) {
                order.push(rid.clone());
            }
            groups.entry(rid.clone()).or_default().push(line.clone());
        }
    }

    order
        .into_iter()
        .filter_map(|rid| {
            groups.remove(&rid).map(|lines| RequestGroup {
                request_id: rid,
                lines,
            })
        })
        .collect()
}

/// L2 验证：检查 requestId 分组内的结构
pub fn validate_request_groups(
    groups: &[RequestGroup],
    session_id: &str,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    for group in groups {
        // 所有行应该是 assistant 类型
        for line in &group.lines {
            if line.entry_type != "assistant" {
                findings.push(Finding {
                    level: Level::L2Block,
                    severity: Severity::Warning,
                    rule_id: "L2-NON-ASSISTANT-IN-GROUP".to_string(),
                    message: format!(
                        "requestId={} 分组中出现非 assistant 行 (type={})",
                        truncate_with_ellipsis(&group.request_id, 16),
                        line.entry_type
                    ),
                    session_id: Some(session_id.to_string()),
                    line_number: Some(line.line_number),
                    context: None,
                });
            }
        }

        // parentUuid 链连续性：组内每行的 parentUuid 应该指向前一行的 uuid
        if group.lines.len() > 1 {
            for i in 1..group.lines.len() {
                let prev_uuid = &group.lines[i - 1].uuid;
                let curr_parent = &group.lines[i].parent_uuid;
                if let (Some(prev), Some(parent)) = (prev_uuid, curr_parent) {
                    if prev != parent {
                        // 允许并行 tool_use 分叉：多个行共享同一个 parentUuid
                        // 这不是错误，只是 info 级别记录
                        findings.push(Finding {
                            level: Level::L2Block,
                            severity: Severity::Info,
                            rule_id: "L2-PARENT-CHAIN-BREAK".to_string(),
                            message: format!(
                                "requestId={} 组内 parentUuid 非连续 (可能是并行 tool_use)",
                                truncate_with_ellipsis(&group.request_id, 16)
                            ),
                            session_id: Some(session_id.to_string()),
                            line_number: Some(group.lines[i].line_number),
                            context: Some(format!(
                                "prev.uuid={}, curr.parentUuid={}",
                                truncate_with_ellipsis(prev, 12),
                                truncate_with_ellipsis(parent, 12)
                            )),
                        });
                    }
                }
            }
        }

        // JSONL 无有效 stop_reason（始终 null），Turn 边界由 content block type 推断

        // usage 存在性检查：至少最后一行应有 usage
        let last_has_usage = group
            .lines
            .last()
            .map(|l| {
                l.value
                    .get("message")
                    .and_then(|m| m.get("usage"))
                    .map(|v| !v.is_null())
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        if !last_has_usage && !group.lines.is_empty() {
            // 不是所有 assistant 响应都有 usage，只做 info 记录
            findings.push(Finding {
                level: Level::L2Block,
                severity: Severity::Info,
                rule_id: "L2-NO-USAGE".to_string(),
                message: format!(
                    "requestId={} 末行无 usage",
                    truncate_with_ellipsis(&group.request_id, 16)
                ),
                session_id: Some(session_id.to_string()),
                line_number: group.lines.last().map(|l| l.line_number),
                context: None,
            });
        }
    }

    findings
}
