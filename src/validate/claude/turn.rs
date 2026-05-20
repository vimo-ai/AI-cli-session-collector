//! L3: Turn 结构验证

use crate::validate::framework::*;

/// 检测 Turn 边界并构建 Turn 结构
///
/// Turn 起始条件：type="user" 且 message.content 为 string（非 tool_result）
pub fn detect_turns(lines: &[ValidatedLine], groups: &[RequestGroup]) -> Vec<Turn> {
    let mut turns = Vec::new();
    let mut current_turn: Option<TurnBuilder> = None;
    let mut turn_index = 0;

    for line in lines {
        let is_user_text = is_user_text_line(line);

        if is_user_text {
            // 结束前一个 Turn
            if let Some(builder) = current_turn.take() {
                turns.push(builder.build(turn_index));
                turn_index += 1;
            }
            // 开始新 Turn
            current_turn = Some(TurnBuilder::new(line.clone()));
        } else if let Some(ref mut builder) = current_turn {
            // 归入当前 Turn
            match line.entry_type.as_str() {
                "assistant" => {
                    // assistant 行通过 requestId 归入对应 group
                    // （groups 已经在 L2 构建好了，这里只记录）
                }
                "user" => {
                    // user 但不是 user_text → tool_result
                    builder.tool_result_lines.push(line.clone());
                }
                "system" => {
                    builder.system_lines.push(line.clone());
                }
                _ => {
                    // summary, custom-title 等放入 system_lines
                    builder.system_lines.push(line.clone());
                }
            }
        }
        // 如果还没有开始任何 Turn（例如开头是 summary），跳过
    }

    // 收尾最后一个 Turn
    if let Some(builder) = current_turn {
        turns.push(builder.build(turn_index));
    }

    // 将 request groups 关联到 turns
    associate_groups_to_turns(&mut turns, groups);

    turns
}

/// 将 request groups 关联到对应的 Turn
fn associate_groups_to_turns(turns: &mut [Turn], groups: &[RequestGroup]) {
    for group in groups {
        if group.lines.is_empty() {
            continue;
        }
        let group_first_line = group.lines[0].line_number;

        // 找到包含此 group 的 Turn（group 首行在 user_line 之后）
        let mut matched = false;
        for turn in turns.iter_mut().rev() {
            if group_first_line > turn.user_line.line_number {
                turn.request_groups.push(group.clone());
                matched = true;
                break;
            }
        }
        if !matched {
            // group 在第一个 Turn 之前，归入第一个 Turn（如果有）
            if let Some(first_turn) = turns.first_mut() {
                first_turn.request_groups.push(group.clone());
            }
        }
    }
}

/// 判断是否为 user text 行（Turn 起始标记）
fn is_user_text_line(line: &ValidatedLine) -> bool {
    if line.entry_type != "user" {
        return false;
    }

    // 有 toolUseResult → 是 tool_result，不是 user text
    if line
        .value
        .get("toolUseResult")
        .map(|v| !v.is_null())
        .unwrap_or(false)
    {
        return false;
    }

    // message.content 是 string → user text
    if let Some(content) = line.value.get("message").and_then(|m| m.get("content")) {
        return content.is_string();
    }

    // isCompactSummary → 不是真正的用户输入
    if line
        .value
        .get("isCompactSummary")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return false;
    }

    // 没有 message 但 type=user（罕见情况），视为 user text
    true
}

/// L3 验证：Turn 生命周期断言
pub fn validate_turns(turns: &[Turn], session_id: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    for turn in turns {
        // Turn 应该至少有一个 request group（用户输入后应有 AI 响应）
        if turn.request_groups.is_empty() {
            findings.push(Finding {
                level: Level::L3Turn,
                severity: Severity::Warning,
                rule_id: "L3-TURN-NO-RESPONSE".to_string(),
                message: format!("Turn {} 无 assistant 响应", turn.turn_index),
                session_id: Some(session_id.to_string()),
                line_number: Some(turn.user_line.line_number),
                context: None,
            });
        }

        // Turn 结束判定：最后一个 assistant 行的 content 不含 tool_use block 即为正常结束
        // JSONL 无有效 stop_reason，从 content block type 推断
        if let Some(last_group) = turn.request_groups.last()
            && let Some(last_line) = last_group.lines.last() {
                let has_tool_use = last_line
                    .value
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                    .map(|blocks| {
                        blocks
                            .iter()
                            .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                    })
                    .unwrap_or(false);

                if has_tool_use && turn.tool_result_lines.is_empty() {
                    findings.push(Finding {
                        level: Level::L3Turn,
                        severity: Severity::Warning,
                        rule_id: "L3-TOOL-USE-NO-RESULT".to_string(),
                        message: format!(
                            "Turn {} 以 tool_use 结束但无 tool_result",
                            turn.turn_index
                        ),
                        session_id: Some(session_id.to_string()),
                        line_number: Some(last_line.line_number),
                        context: None,
                    });
                }
            }

        // system 行检查：turn_duration 通常在 Turn 末尾
        let has_turn_duration = turn
            .system_lines
            .iter()
            .any(|l| l.value.get("subtype").and_then(|v| v.as_str()) == Some("turn_duration"));

        if !has_turn_duration && !turn.request_groups.is_empty() {
            // 不是所有 Turn 都有 turn_duration（早期版本可能没有）
            // 只做 info 记录
            findings.push(Finding {
                level: Level::L3Turn,
                severity: Severity::Info,
                rule_id: "L3-NO-TURN-DURATION".to_string(),
                message: format!("Turn {} 缺少 turn_duration", turn.turn_index),
                session_id: Some(session_id.to_string()),
                line_number: Some(turn.user_line.line_number),
                context: None,
            });
        }
    }

    findings
}

/// Turn 构建器
struct TurnBuilder {
    user_line: ValidatedLine,
    tool_result_lines: Vec<ValidatedLine>,
    system_lines: Vec<ValidatedLine>,
}

impl TurnBuilder {
    fn new(user_line: ValidatedLine) -> Self {
        Self {
            user_line,
            tool_result_lines: Vec::new(),
            system_lines: Vec::new(),
        }
    }

    fn build(self, turn_index: usize) -> Turn {
        Turn {
            turn_index,
            user_line: self.user_line,
            request_groups: Vec::new(), // 由 associate_groups_to_turns 填充
            tool_result_lines: self.tool_result_lines,
            system_lines: self.system_lines,
        }
    }
}
