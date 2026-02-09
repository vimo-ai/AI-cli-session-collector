//! L5: 项目级文件一致性验证

use crate::validate::framework::*;
use std::collections::HashSet;
use std::path::Path;

use super::super::framework::truncate;

/// 验证 sessions-index.json 一致性
pub fn validate_sessions_index(
    project_dir: &Path,
    session_files: &[String], // session IDs from JSONL files
) -> Vec<Finding> {
    let mut findings = Vec::new();

    let index_path = project_dir.join("sessions-index.json");
    if !index_path.exists() {
        // 没有 index 文件是允许的（旧版 Claude Code）
        findings.push(Finding {
            level: Level::L5Project,
            severity: Severity::Info,
            rule_id: "L5-NO-INDEX".to_string(),
            message: "项目目录无 sessions-index.json".to_string(),
            session_id: None,
            line_number: None,
            context: Some(project_dir.display().to_string()),
        });
        return findings;
    }

    // 读取并解析 index
    let content = match std::fs::read_to_string(&index_path) {
        Ok(c) => c,
        Err(e) => {
            findings.push(Finding {
                level: Level::L5Project,
                severity: Severity::Error,
                rule_id: "L5-INDEX-READ-ERROR".to_string(),
                message: format!("无法读取 sessions-index.json: {}", e),
                session_id: None,
                line_number: None,
                context: None,
            });
            return findings;
        }
    };

    let index: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            findings.push(Finding {
                level: Level::L5Project,
                severity: Severity::Error,
                rule_id: "L5-INDEX-PARSE-ERROR".to_string(),
                message: format!("sessions-index.json JSON 解析失败: {}", e),
                session_id: None,
                line_number: None,
                context: None,
            });
            return findings;
        }
    };

    // 检查 index 字段
    if let Some(obj) = index.as_object() {
        let known_keys: HashSet<&str> =
            ["version", "entries", "originalPath"].into_iter().collect();
        for key in obj.keys() {
            if !known_keys.contains(key.as_str()) {
                findings.push(Finding {
                    level: Level::L5Project,
                    severity: Severity::Warning,
                    rule_id: "L5-INDEX-UNKNOWN-FIELD".to_string(),
                    message: format!("sessions-index.json 未知字段: {}", key),
                    session_id: None,
                    line_number: None,
                    context: None,
                });
            }
        }
    }

    // 提取 index 中的 sessionIds
    let index_ids: HashSet<String> = index
        .get("entries")
        .and_then(|e| e.as_array())
        .map(|entries| {
            // 验证 entry 字段
            for entry in entries {
                if let Some(entry_obj) = entry.as_object() {
                    let known: HashSet<&str> = super::schema::INDEX_ENTRY_FIELDS
                        .iter()
                        .copied()
                        .collect();
                    for key in entry_obj.keys() {
                        if !known.contains(key.as_str()) {
                            findings.push(Finding {
                                level: Level::L5Project,
                                severity: Severity::Warning,
                                rule_id: "L5-INDEX-ENTRY-UNKNOWN-FIELD".to_string(),
                                message: format!(
                                    "sessions-index entry 未知字段: {}",
                                    key
                                ),
                                session_id: entry_obj
                                    .get("sessionId")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                                line_number: None,
                                context: None,
                            });
                        }
                    }
                }
            }

            entries
                .iter()
                .filter_map(|e| e.get("sessionId").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let disk_ids: HashSet<String> = session_files.iter().cloned().collect();

    // 在 index 中但磁盘上没有（可能已过期被删除，正常）
    let index_only: Vec<_> = index_ids.difference(&disk_ids).collect();
    if !index_only.is_empty() {
        findings.push(Finding {
            level: Level::L5Project,
            severity: Severity::Info,
            rule_id: "L5-INDEX-ORPHAN".to_string(),
            message: format!(
                "index 中有 {} 个 session 磁盘上不存在（可能已过期）",
                index_only.len()
            ),
            session_id: None,
            line_number: None,
            context: Some(
                index_only
                    .iter()
                    .take(3)
                    .map(|s| truncate(s, 12).to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        });
    }

    // 磁盘上有但 index 中没有
    let disk_only: Vec<_> = disk_ids.difference(&index_ids).collect();
    if !disk_only.is_empty() {
        findings.push(Finding {
            level: Level::L5Project,
            severity: Severity::Warning,
            rule_id: "L5-INDEX-MISSING".to_string(),
            message: format!(
                "磁盘有 {} 个 session 不在 index 中",
                disk_only.len()
            ),
            session_id: None,
            line_number: None,
            context: Some(
                disk_only
                    .iter()
                    .take(3)
                    .map(|s| truncate(s, 12).to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        });
    }

    findings
}

/// 检测项目目录下的未知文件/目录
pub fn detect_unknown_files(project_dir: &Path, session_ids: &[String]) -> Vec<Finding> {
    let mut findings = Vec::new();

    let entries = match std::fs::read_dir(project_dir) {
        Ok(e) => e,
        Err(_) => return findings,
    };

    let session_set: HashSet<&str> = session_ids.iter().map(|s| s.as_str()).collect();
    let allowed_files: HashSet<&str> = super::schema::ALLOWED_PROJECT_FILES
        .iter()
        .copied()
        .collect();

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry
            .file_name()
            .to_str()
            .unwrap_or_default()
            .to_string();

        if name.starts_with('.') {
            continue; // 隐藏文件跳过
        }

        if path.is_file() {
            if name.ends_with(".jsonl") {
                // JSONL 文件应该是已知 session
                let stem = name.trim_end_matches(".jsonl");
                if !session_set.contains(stem) {
                    // 这不应该发生，因为 session_ids 就是从这些文件提取的
                    // 但以防万一
                }
            } else if !allowed_files.contains(name.as_str()) {
                findings.push(Finding {
                    level: Level::L5Project,
                    severity: Severity::Warning,
                    rule_id: "L5-UNKNOWN-FILE".to_string(),
                    message: format!("项目目录下未知文件: {}", name),
                    session_id: None,
                    line_number: None,
                    context: Some(project_dir.display().to_string()),
                });
            }
        } else if path.is_dir() {
            // 目录应该是 session 附属目录（UUID 命名）
            if !session_set.contains(name.as_str()) {
                findings.push(Finding {
                    level: Level::L5Project,
                    severity: Severity::Warning,
                    rule_id: "L5-UNKNOWN-DIR".to_string(),
                    message: format!("项目目录下未知目录: {}", name),
                    session_id: None,
                    line_number: None,
                    context: Some(project_dir.display().to_string()),
                });
            } else {
                // 检查附属目录内的子目录
                let sub_entries = match std::fs::read_dir(&path) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                let allowed_subdirs: HashSet<&str> =
                    super::schema::ALLOWED_SESSION_SUBDIRS.iter().copied().collect();
                for sub_entry in sub_entries.flatten() {
                    if sub_entry.path().is_dir() {
                        let sub_name = sub_entry
                            .file_name()
                            .to_str()
                            .unwrap_or_default()
                            .to_string();
                        if !allowed_subdirs.contains(sub_name.as_str()) {
                            findings.push(Finding {
                                level: Level::L5Project,
                                severity: Severity::Warning,
                                rule_id: "L5-UNKNOWN-SESSION-SUBDIR".to_string(),
                                message: format!(
                                    "session 附属目录下未知子目录: {}/{}",
                                    name, sub_name
                                ),
                                session_id: Some(name.clone()),
                                line_number: None,
                                context: None,
                            });
                        }
                    }
                }
            }
        }
    }

    findings
}

/// 验证 tool-results 交叉引用
pub fn validate_tool_results(
    session_dir: &Path,
    lines: &[ValidatedLine],
    session_id: &str,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    let tool_results_dir = session_dir.join("tool-results");
    if !tool_results_dir.exists() {
        return findings;
    }

    // 收集磁盘上的 tool-result 文件
    let disk_files: HashSet<String> = match std::fs::read_dir(&tool_results_dir) {
        Ok(entries) => entries
            .flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .collect(),
        Err(_) => return findings,
    };

    // 收集 JSONL 中引用的 tool-result 文件（通过 <persisted-output> 标记）
    let mut referenced_files: HashSet<String> = HashSet::new();
    for line in lines {
        let json_str = line.value.to_string();
        // 查找 tool-results/ 引用
        if json_str.contains("tool-results/") {
            // 简单提取：找 tool-results/后面的文件名
            for part in json_str.split("tool-results/") {
                if let Some(end) = part.find(|c: char| c == '"' || c == '\'' || c == '\\') {
                    let filename = &part[..end];
                    if !filename.is_empty() {
                        referenced_files.insert(filename.to_string());
                    }
                }
            }
        }
    }

    // 磁盘有但 JSONL 未引用
    let orphan: Vec<_> = disk_files.difference(&referenced_files).collect();
    if !orphan.is_empty() {
        findings.push(Finding {
            level: Level::L5Project,
            severity: Severity::Info,
            rule_id: "L5-TOOL-RESULT-ORPHAN".to_string(),
            message: format!(
                "{} 个 tool-result 文件在 JSONL 中未被引用",
                orphan.len()
            ),
            session_id: Some(session_id.to_string()),
            line_number: None,
            context: Some(
                orphan
                    .iter()
                    .take(3)
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        });
    }

    // JSONL 引用但磁盘无
    let missing: Vec<_> = referenced_files.difference(&disk_files).collect();
    if !missing.is_empty() {
        findings.push(Finding {
            level: Level::L5Project,
            severity: Severity::Warning,
            rule_id: "L5-TOOL-RESULT-MISSING".to_string(),
            message: format!(
                "{} 个 tool-result 文件被 JSONL 引用但不存在",
                missing.len()
            ),
            session_id: Some(session_id.to_string()),
            line_number: None,
            context: Some(
                missing
                    .iter()
                    .take(3)
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        });
    }

    findings
}
