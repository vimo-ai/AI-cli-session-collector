//! L4: Session 结构验证

use crate::validate::framework::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::super::framework::truncate;

/// 构建 parentUuid 树，返回 (children_map, root_uuids)
pub fn build_uuid_tree(lines: &[ValidatedLine]) -> (HashMap<String, Vec<String>>, Vec<String>) {
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    let mut all_uuids: HashSet<String> = HashSet::new();
    let mut has_parent: HashSet<String> = HashSet::new();

    for line in lines {
        if let Some(uuid) = &line.uuid {
            all_uuids.insert(uuid.clone());
            if let Some(parent) = &line.parent_uuid {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(uuid.clone());
                has_parent.insert(uuid.clone());
            }
        }
    }

    let roots: Vec<String> = all_uuids.difference(&has_parent).cloned().collect();

    (children, roots)
}

/// 检测 fork 点（parentUuid 被多个 uuid 引用）
pub fn detect_forks(lines: &[ValidatedLine]) -> Vec<(String, usize)> {
    let mut parent_ref_count: HashMap<String, usize> = HashMap::new();
    for line in lines {
        if let Some(parent) = &line.parent_uuid {
            *parent_ref_count.entry(parent.clone()).or_default() += 1;
        }
    }
    parent_ref_count
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .collect()
}

/// 验证 compaction 链
pub fn validate_compaction_chains(lines: &[ValidatedLine], session_id: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let is_compact = lines[i]
            .value
            .get("subtype")
            .and_then(|v| v.as_str())
            .map(|s| s == "compact_boundary" || s == "microcompact_boundary")
            .unwrap_or(false);

        if is_compact {
            // 下一行应该是 isCompactSummary=true 的 user 消息
            if i + 1 < lines.len() {
                let next = &lines[i + 1];
                let is_summary = next
                    .value
                    .get("isCompactSummary")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if !is_summary {
                    findings.push(Finding {
                        level: Level::L4Session,
                        severity: Severity::Warning,
                        rule_id: "L4-COMPACT-NO-SUMMARY".to_string(),
                        message: "compact_boundary 后缺少 isCompactSummary 行".to_string(),
                        session_id: Some(session_id.to_string()),
                        line_number: Some(lines[i].line_number),
                        context: None,
                    });
                }
            } else {
                findings.push(Finding {
                    level: Level::L4Session,
                    severity: Severity::Warning,
                    rule_id: "L4-COMPACT-AT-END".to_string(),
                    message: "compact_boundary 出现在文件末尾（无后续摘要）".to_string(),
                    session_id: Some(session_id.to_string()),
                    line_number: Some(lines[i].line_number),
                    context: None,
                });
            }

            // logicalParentUuid 应指向本 session 中存在的 uuid
            if let Some(logical_parent) = lines[i]
                .value
                .get("logicalParentUuid")
                .and_then(|v| v.as_str())
            {
                let exists = lines
                    .iter()
                    .any(|l| l.uuid.as_deref() == Some(logical_parent));
                if !exists {
                    findings.push(Finding {
                        level: Level::L4Session,
                        severity: Severity::Warning,
                        rule_id: "L4-COMPACT-ORPHAN-LOGICAL-PARENT".to_string(),
                        message: format!(
                            "logicalParentUuid {} 在本 session 中不存在",
                            truncate(logical_parent, 12)
                        ),
                        session_id: Some(session_id.to_string()),
                        line_number: Some(lines[i].line_number),
                        context: None,
                    });
                }
            }
        }

        i += 1;
    }

    findings
}

/// 验证 subagent 目录
pub fn validate_subagent_dirs(session_dir: &Path, session_id: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    let subagents_dir = session_dir.join("subagents");
    if !subagents_dir.exists() {
        return findings;
    }

    let entries = match std::fs::read_dir(&subagents_dir) {
        Ok(e) => e,
        Err(_) => return findings,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();

        if !filename.ends_with(".jsonl") {
            findings.push(Finding {
                level: Level::L4Session,
                severity: Severity::Warning,
                rule_id: "L4-SUBAGENT-UNKNOWN-FILE".to_string(),
                message: format!("subagents/ 下非 JSONL 文件: {}", filename),
                session_id: Some(session_id.to_string()),
                line_number: None,
                context: None,
            });
            continue;
        }

        // 命名模式检查
        let valid_pattern = filename.starts_with("agent-a");
        if !valid_pattern {
            findings.push(Finding {
                level: Level::L4Session,
                severity: Severity::Warning,
                rule_id: "L4-SUBAGENT-BAD-NAME".to_string(),
                message: format!("subagent 文件名不符合 agent-a* 模式: {}", filename),
                session_id: Some(session_id.to_string()),
                line_number: None,
                context: None,
            });
        }

        // 检查 subagent JSONL 的第一行是否有 isSidechain=true
        if let Ok(file) = std::fs::File::open(&path) {
            use std::io::{BufRead, BufReader};
            let reader = BufReader::new(file);
            if let Some(Ok(first_line)) = reader.lines().next()
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(&first_line) {
                    let is_sidechain = val
                        .get("isSidechain")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if !is_sidechain {
                        findings.push(Finding {
                            level: Level::L4Session,
                            severity: Severity::Warning,
                            rule_id: "L4-SUBAGENT-NOT-SIDECHAIN".to_string(),
                            message: format!("subagent {} 的 isSidechain 不为 true", filename),
                            session_id: Some(session_id.to_string()),
                            line_number: None,
                            context: None,
                        });
                    }

                    // sessionId 应匹配父 session
                    if let Some(sid) = val.get("sessionId").and_then(|v| v.as_str())
                        && sid != session_id {
                            findings.push(Finding {
                                level: Level::L4Session,
                                severity: Severity::Warning,
                                rule_id: "L4-SUBAGENT-SESSION-MISMATCH".to_string(),
                                message: format!(
                                    "subagent {} 的 sessionId ({}) 不匹配父 session ({})",
                                    filename,
                                    truncate(sid, 12),
                                    truncate(session_id, 12)
                                ),
                                session_id: Some(session_id.to_string()),
                                line_number: None,
                                context: None,
                            });
                        }
                }
        }
    }

    findings
}

/// L4 综合验证
pub fn validate_session(
    lines: &[ValidatedLine],
    session_id: &str,
    session_dir: Option<&Path>,
) -> (Vec<Finding>, usize, usize) {
    let mut findings = Vec::new();

    // 构建 UUID 树
    let (_children, roots) = build_uuid_tree(lines);

    // 多根检查（允许 compaction 后的多根）
    let compaction_count = lines
        .iter()
        .filter(|l| {
            l.value
                .get("subtype")
                .and_then(|v| v.as_str())
                .map(|s| s == "compact_boundary" || s == "microcompact_boundary")
                .unwrap_or(false)
        })
        .count();

    // 每次 compaction 会引入一个新的根节点
    let expected_max_roots = 1 + compaction_count;
    if roots.len() > expected_max_roots + 1 {
        // +1 容差：summary/custom-title 可能没有 parentUuid
        findings.push(Finding {
            level: Level::L4Session,
            severity: Severity::Info,
            rule_id: "L4-MULTIPLE-ROOTS".to_string(),
            message: format!(
                "UUID 树有 {} 个根节点（compaction {} 次，期望最多 {}）",
                roots.len(),
                compaction_count,
                expected_max_roots
            ),
            session_id: Some(session_id.to_string()),
            line_number: None,
            context: None,
        });
    }

    // fork 检测
    let forks = detect_forks(lines);
    let fork_count = forks.len();

    // compaction 链验证
    let compact_findings = validate_compaction_chains(lines, session_id);
    findings.extend(compact_findings);

    // subagent 目录验证
    if let Some(dir) = session_dir {
        let sub_findings = validate_subagent_dirs(dir, session_id);
        findings.extend(sub_findings);
    }

    (findings, fork_count, compaction_count)
}
