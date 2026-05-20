//! Claude Code 验证器编排
//!
//! 按顺序执行 L0→L5 验证

pub mod block;
pub mod line;
pub mod project;
pub mod schema;
pub mod session;
pub mod turn;

use crate::validate::framework::*;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::framework::truncate;

/// Claude Code 验证器
pub struct ClaudeValidator {
    /// Claude projects 目录
    projects_path: PathBuf,
    /// 最大验证层级
    max_level: u8,
    /// 单 session 模式
    single_session: Option<PathBuf>,
}

impl ClaudeValidator {
    pub fn new(projects_path: PathBuf, max_level: u8) -> Self {
        Self {
            projects_path,
            max_level,
            single_session: None,
        }
    }

    pub fn with_single_session(mut self, path: PathBuf) -> Self {
        self.single_session = Some(path);
        self
    }

    /// 执行全部验证
    pub fn run(&self) -> ValidationResult {
        let mut result = ValidationResult::new();

        if let Some(session_path) = &self.single_session {
            // 单 session 模式
            let session_id = session_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            let session_dir = session_path.parent().map(|p| p.join(&session_id));

            self.validate_session_file(
                session_path,
                &session_id,
                session_dir.as_deref(),
                &mut result,
            );
            result.stats.session_count = 1;
            result.stats.project_count = 1;
            return result;
        }

        // 全量模式：遍历项目目录
        if !self.projects_path.exists() {
            result.findings.push(Finding {
                level: Level::L5Project,
                severity: Severity::Error,
                rule_id: "L5-PATH-NOT-FOUND".to_string(),
                message: format!("projects 目录不存在: {}", self.projects_path.display()),
                session_id: None,
                line_number: None,
                context: None,
            });
            return result;
        }

        let project_dirs = match std::fs::read_dir(&self.projects_path) {
            Ok(entries) => entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .filter(|e| !e.file_name().to_str().unwrap_or_default().starts_with('.'))
                .collect::<Vec<_>>(),
            Err(e) => {
                result.findings.push(Finding {
                    level: Level::L5Project,
                    severity: Severity::Error,
                    rule_id: "L5-PATH-READ-ERROR".to_string(),
                    message: format!("无法读取 projects 目录: {}", e),
                    session_id: None,
                    line_number: None,
                    context: None,
                });
                return result;
            }
        };

        result.stats.project_count = project_dirs.len();

        for project_entry in &project_dirs {
            let project_dir = project_entry.path();
            self.validate_project_dir(&project_dir, &mut result);
        }

        result
    }

    /// 验证单个项目目录
    fn validate_project_dir(&self, project_dir: &Path, result: &mut ValidationResult) {
        // 收集 JSONL 文件
        let entries = match std::fs::read_dir(project_dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        let mut session_ids = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_str().unwrap_or_default().to_string();
            if !name.ends_with(".jsonl") {
                continue;
            }
            let session_id = name.trim_end_matches(".jsonl").to_string();
            session_ids.push(session_id.clone());

            let session_dir = project_dir.join(&session_id);
            let session_dir_opt = if session_dir.is_dir() {
                Some(session_dir.as_path())
            } else {
                None
            };

            self.validate_session_file(&path, &session_id, session_dir_opt, result);
            result.stats.session_count += 1;

            // 统计 subagent 数量
            if let Some(dir) = session_dir_opt {
                let subagent_dir = dir.join("subagents");
                if subagent_dir.is_dir()
                    && let Ok(sub_entries) = std::fs::read_dir(&subagent_dir) {
                        result.stats.subagent_count += sub_entries
                            .flatten()
                            .filter(|e| {
                                e.path().is_file()
                                    && e.file_name()
                                        .to_str()
                                        .unwrap_or_default()
                                        .ends_with(".jsonl")
                            })
                            .count();
                    }
            }
        }

        // L5 验证
        if self.max_level >= 5 {
            let index_findings = project::validate_sessions_index(project_dir, &session_ids);
            result.findings.extend(index_findings);

            let unknown_findings = project::detect_unknown_files(project_dir, &session_ids);
            result.findings.extend(unknown_findings);
        }
    }

    /// 验证单个 session 文件（L0 → L4）
    fn validate_session_file(
        &self,
        file_path: &Path,
        session_id: &str,
        session_dir: Option<&Path>,
        result: &mut ValidationResult,
    ) {
        let file = match std::fs::File::open(file_path) {
            Ok(f) => f,
            Err(e) => {
                result.findings.push(Finding {
                    level: Level::L0Field,
                    severity: Severity::Error,
                    rule_id: "L0-FILE-OPEN-ERROR".to_string(),
                    message: format!("无法打开文件: {}", e),
                    session_id: Some(session_id.to_string()),
                    line_number: None,
                    context: Some(file_path.display().to_string()),
                });
                return;
            }
        };

        let reader = BufReader::new(file);
        let mut validated_lines = Vec::new();

        // L0 + L1: 逐行验证
        for (line_idx, line_result) in reader.lines().enumerate() {
            let line_number = line_idx + 1;
            let raw_line = match line_result {
                Ok(l) => l,
                Err(e) => {
                    result.findings.push(Finding {
                        level: Level::L0Field,
                        severity: Severity::Error,
                        rule_id: "L0-LINE-READ-ERROR".to_string(),
                        message: format!("读取第 {} 行失败: {}", line_number, e),
                        session_id: Some(session_id.to_string()),
                        line_number: Some(line_number),
                        context: None,
                    });
                    continue;
                }
            };

            if raw_line.trim().is_empty() {
                continue;
            }

            let value: serde_json::Value = match serde_json::from_str(&raw_line) {
                Ok(v) => v,
                Err(e) => {
                    result.findings.push(Finding {
                        level: Level::L0Field,
                        severity: Severity::Error,
                        rule_id: "L0-JSON-PARSE-ERROR".to_string(),
                        message: format!("JSON 解析失败: {}", e),
                        session_id: Some(session_id.to_string()),
                        line_number: Some(line_number),
                        context: Some(truncate(&raw_line, 80).to_string()),
                    });
                    continue;
                }
            };

            let obj = match value.as_object() {
                Some(o) => o,
                None => {
                    result.findings.push(Finding {
                        level: Level::L0Field,
                        severity: Severity::Error,
                        rule_id: "L0-NOT-OBJECT".to_string(),
                        message: "JSONL 行不是 JSON object".to_string(),
                        session_id: Some(session_id.to_string()),
                        line_number: Some(line_number),
                        context: None,
                    });
                    continue;
                }
            };

            result.stats.total_lines += 1;

            // L0: 字段白名单
            let unknown_fields =
                schema::check_field_whitelist(obj, schema::CLAUDE_TOP_LEVEL_FIELDS);
            for field_name in &unknown_fields {
                let info = result
                    .stats
                    .unknown_fields
                    .entry(field_name.clone())
                    .or_insert_with(|| UnknownFieldInfo {
                        count: 0,
                        first_session: Some(session_id.to_string()),
                        first_line: Some(line_number),
                        sample_value: obj.get(field_name).map(|v| {
                            let s = v.to_string();
                            if truncate(&s, 60).len() < s.len() {
                                format!("{}...", truncate(&s, 60))
                            } else {
                                s
                            }
                        }),
                    });
                info.count += 1;
            }

            // L0: 类型检查
            let type_mismatches = schema::check_field_types(obj, schema::CLAUDE_TOP_LEVEL_FIELDS);
            for (fname, expected, actual) in &type_mismatches {
                result.findings.push(Finding {
                    level: Level::L0Field,
                    severity: Severity::Error,
                    rule_id: "L0-TYPE-MISMATCH".to_string(),
                    message: format!("字段 \"{}\": 期望 {}, 实际 {}", fname, expected, actual),
                    session_id: Some(session_id.to_string()),
                    line_number: Some(line_number),
                    context: None,
                });
            }

            // L0: 值域检查
            let domain_violations =
                schema::check_value_domains(obj, schema::CLAUDE_TOP_LEVEL_FIELDS);
            for (fname, violation) in &domain_violations {
                result.findings.push(Finding {
                    level: Level::L0Field,
                    severity: Severity::Warning,
                    rule_id: "L0-VALUE-DOMAIN".to_string(),
                    message: format!("字段 \"{}\": {}", fname, violation),
                    session_id: Some(session_id.to_string()),
                    line_number: Some(line_number),
                    context: None,
                });
            }

            // L0: message 子字段检查
            if let Some(message) = obj.get("message").and_then(|v| v.as_object()) {
                let msg_unknown = schema::check_field_whitelist(message, schema::MESSAGE_FIELDS);
                for field_name in &msg_unknown {
                    let key = format!("message.{}", field_name);
                    let info = result
                        .stats
                        .unknown_fields
                        .entry(key.clone())
                        .or_insert_with(|| UnknownFieldInfo {
                            count: 0,
                            first_session: Some(session_id.to_string()),
                            first_line: Some(line_number),
                            sample_value: message.get(field_name).map(|v| {
                                let s = v.to_string();
                                if truncate(&s, 60).len() < s.len() {
                                    format!("{}...", truncate(&s, 60))
                                } else {
                                    s
                                }
                            }),
                        });
                    info.count += 1;
                }

                // L0: content block 检查
                let block_findings =
                    line::validate_message_content_blocks(line_number, message, session_id);
                result.findings.extend(block_findings);
            }

            // 提取 entry_type
            let entry_type = obj
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            // 统计 type 分布
            *result
                .stats
                .type_counts
                .entry(entry_type.clone())
                .or_default() += 1;

            // 已知字段计数（非未知的字段数）
            result.stats.known_field_count = schema::CLAUDE_TOP_LEVEL_FIELDS.len();

            // L1: 按 type 的字段组合检查
            if self.max_level >= 1 {
                let l1_findings = line::validate_line(line_number, &entry_type, obj, session_id);
                result.findings.extend(l1_findings);
            }

            // 构建 ValidatedLine 供 L2+ 使用
            validated_lines.push(ValidatedLine {
                line_number,
                entry_type,
                uuid: obj
                    .get("uuid")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                parent_uuid: obj
                    .get("parentUuid")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                request_id: obj
                    .get("requestId")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                session_id: obj
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                value,
            });
        }

        // 如果没有 validated_lines，后续层级跳过
        if validated_lines.is_empty() {
            return;
        }

        // L2: requestId 分组验证
        if self.max_level >= 2 {
            let groups = block::group_by_request_id(&validated_lines);
            result.stats.request_group_count += groups.len();

            let l2_findings = block::validate_request_groups(&groups, session_id);
            result.findings.extend(l2_findings);

            // L3: Turn 结构验证
            if self.max_level >= 3 {
                let turns = turn::detect_turns(&validated_lines, &groups);
                result.stats.turn_count += turns.len();

                let l3_findings = turn::validate_turns(&turns, session_id);
                result.findings.extend(l3_findings);
            }
        }

        // L4: Session 结构验证
        if self.max_level >= 4 {
            let (l4_findings, fork_count, compaction_count) =
                session::validate_session(&validated_lines, session_id, session_dir);
            result.stats.fork_count += fork_count;
            result.stats.compaction_count += compaction_count;
            result.findings.extend(l4_findings);

            // L5: tool-results 交叉引用
            if self.max_level >= 5
                && let Some(dir) = session_dir {
                    let tr_findings =
                        project::validate_tool_results(dir, &validated_lines, session_id);
                    result.findings.extend(tr_findings);
                }
        }
    }
}
