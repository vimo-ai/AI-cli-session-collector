//! 验证报告格式化输出

use super::framework::*;
use std::collections::HashMap;

/// 输出格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
}

/// 生成人类可读报告
pub fn format_human_report(result: &ValidationResult, verbose: bool) -> String {
    let mut out = String::new();

    out.push_str("Claude Code 结构验证 (~/.claude/projects/)\n");
    out.push_str(&"━".repeat(50));
    out.push('\n');

    let stats = &result.stats;
    out.push_str(&format!(
        "扫描: {} 个项目, {} 个主 session, {} 个 subagent\n\n",
        stats.project_count, stats.session_count, stats.subagent_count
    ));

    // 按层级汇总
    let mut by_level: HashMap<Level, Vec<&Finding>> = HashMap::new();
    for f in &result.findings {
        by_level.entry(f.level).or_default().push(f);
    }

    let levels = [
        Level::L0Field,
        Level::L1Line,
        Level::L2Block,
        Level::L3Turn,
        Level::L4Session,
        Level::L5Project,
    ];

    for level in &levels {
        let findings = by_level.get(level).map(|v| v.as_slice()).unwrap_or(&[]);
        let errors: Vec<_> = findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .collect();
        let warnings: Vec<_> = findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .collect();
        let infos: Vec<_> = findings
            .iter()
            .filter(|f| f.severity == Severity::Info)
            .collect();

        // Level header with stats
        match level {
            Level::L0Field => {
                out.push_str(&format!("{:<12} ", level.to_string()));
                if errors.is_empty() && warnings.is_empty() {
                    out.push_str(&format!(
                        "\x1b[32m✓\x1b[0m {} 行, {} 种已知字段\n",
                        stats.total_lines, stats.known_field_count
                    ));
                } else {
                    out.push_str(&format!(
                        "\x1b[32m✓\x1b[0m {} 行, {} 种已知字段\n",
                        stats.total_lines, stats.known_field_count
                    ));
                }
                if !stats.unknown_fields.is_empty() {
                    let mut sorted: Vec<_> = stats.unknown_fields.iter().collect();
                    sorted.sort_by(|a, b| b.1.count.cmp(&a.1.count));
                    out.push_str(&format!(
                        "             \x1b[33m⚠\x1b[0m {} 个未知字段:",
                        sorted.len()
                    ));
                    for (name, info) in sorted.iter().take(5) {
                        out.push_str(&format!(" \"{}\" ({} 次)", name, info.count));
                    }
                    out.push('\n');
                }
            }
            Level::L1Line => {
                out.push_str(&format!("{:<12} ", level.to_string()));
                if errors.is_empty() {
                    out.push_str(&format!(
                        "\x1b[32m✓\x1b[0m {} 种 type 已覆盖\n",
                        stats.type_counts.len()
                    ));
                } else {
                    out.push_str(&format!("\x1b[31m✗\x1b[0m {} 个错误\n", errors.len()));
                }
            }
            Level::L2Block => {
                out.push_str(&format!("{:<12} ", level.to_string()));
                if errors.is_empty() {
                    out.push_str(&format!(
                        "\x1b[32m✓\x1b[0m {} 个 requestId 分组\n",
                        stats.request_group_count
                    ));
                } else {
                    out.push_str(&format!(
                        "\x1b[31m✗\x1b[0m {} 个错误 ({} 个 requestId 分组)\n",
                        errors.len(),
                        stats.request_group_count
                    ));
                }
            }
            Level::L3Turn => {
                out.push_str(&format!("{:<12} ", level.to_string()));
                if errors.is_empty() {
                    out.push_str(&format!("\x1b[32m✓\x1b[0m {} 个 Turn\n", stats.turn_count));
                } else {
                    out.push_str(&format!(
                        "\x1b[31m✗\x1b[0m {} 个错误 ({} 个 Turn)\n",
                        errors.len(),
                        stats.turn_count
                    ));
                }
                if !warnings.is_empty() {
                    out.push_str(&format!(
                        "             \x1b[33m⚠\x1b[0m {} 个警告\n",
                        warnings.len()
                    ));
                }
            }
            Level::L4Session => {
                out.push_str(&format!("{:<12} ", level.to_string()));
                if errors.is_empty() {
                    out.push_str(&format!(
                        "\x1b[32m✓\x1b[0m {} 个 fork 点, {} 个 compaction 链\n",
                        stats.fork_count, stats.compaction_count
                    ));
                } else {
                    out.push_str(&format!("\x1b[31m✗\x1b[0m {} 个错误\n", errors.len()));
                }
            }
            Level::L5Project => {
                out.push_str(&format!("{:<12} ", level.to_string()));
                if errors.is_empty() && warnings.is_empty() {
                    out.push_str("\x1b[32m✓\x1b[0m 项目结构一致\n");
                } else {
                    if !errors.is_empty() {
                        out.push_str(&format!("\x1b[31m✗\x1b[0m {} 个错误\n", errors.len()));
                    }
                    if !warnings.is_empty() {
                        out.push_str(&format!(
                            "             \x1b[33m⚠\x1b[0m {} 个警告\n",
                            warnings.len()
                        ));
                    }
                }
            }
        }

        // Verbose: 列出每条 finding
        if verbose {
            for f in findings {
                let icon = match f.severity {
                    Severity::Error => "\x1b[31m✗\x1b[0m",
                    Severity::Warning => "\x1b[33m⚠\x1b[0m",
                    Severity::Info => "\x1b[34mℹ\x1b[0m",
                };
                out.push_str(&format!("  {} [{}] {}", icon, f.rule_id, f.message));
                if let Some(sid) = &f.session_id {
                    out.push_str(&format!(" (session: {})", truncate_id(sid)));
                }
                if let Some(ln) = f.line_number {
                    out.push_str(&format!(" :L{}", ln));
                }
                out.push('\n');
                if let Some(ctx) = &f.context {
                    out.push_str(&format!("    {}\n", truncate_str(ctx, 120)));
                }
            }
        }

        // 非 verbose 模式下，Info 级别的数量
        if !verbose && !infos.is_empty() {
            out.push_str(&format!(
                "             \x1b[34mℹ\x1b[0m {} 条信息（使用 --verbose 查看详情）\n",
                infos.len()
            ));
        }
    }

    // 未知字段详表
    if !stats.unknown_fields.is_empty() {
        out.push('\n');
        out.push_str("未知字段详情:\n");
        out.push_str(&format!(
            "  {:<24} {:<8} {:<28} {}\n",
            "字段名", "次数", "首见 session", "示例值"
        ));
        let mut sorted: Vec<_> = stats.unknown_fields.iter().collect();
        sorted.sort_by(|a, b| b.1.count.cmp(&a.1.count));
        for (name, info) in &sorted {
            let session = info
                .first_session
                .as_deref()
                .map(truncate_id)
                .unwrap_or_default();
            let line_suffix = info
                .first_line
                .map(|l| format!(":{}", l))
                .unwrap_or_default();
            let sample = info
                .sample_value
                .as_deref()
                .map(|s| truncate_str(s, 40))
                .unwrap_or_default();
            out.push_str(&format!(
                "  {:<24} {:<8} {:<28} {}\n",
                name,
                info.count,
                format!("{}{}", session, line_suffix),
                sample
            ));
        }
    }

    // 总结
    out.push('\n');
    let total_errors = result.error_count();
    let total_warnings = result.warning_count();
    if total_errors == 0 && total_warnings == 0 {
        out.push_str("\x1b[32m所有验证通过\x1b[0m\n");
    } else {
        out.push_str(&format!(
            "总计: {} 个错误, {} 个警告\n",
            total_errors, total_warnings
        ));
    }

    out
}

/// 生成 JSON 报告
pub fn format_json_report(result: &ValidationResult) -> String {
    serde_json::to_string_pretty(result).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
}

fn truncate_id(s: &str) -> String {
    let truncated: String = s.chars().take(12).collect();
    if s.chars().count() > 12 {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    let truncated: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        format!("{}...", truncated)
    } else {
        truncated
    }
}
