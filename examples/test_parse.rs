//! 测试 Claude Code 解析

use ai_cli_session_collector::{ClaudeAdapter, CodexAdapter, ConversationAdapter};
use std::path::PathBuf;

fn main() {
    // 初始化 tracing
    tracing_subscriber::fmt::init();

    println!("=== 测试 ai-cli-session-collector 解析 ===\n");

    // 先测试包含 thinking 的文件
    let thinking_file = "/Users/higuaifan/.claude/projects/-Users-higuaifan-Desktop-hi-----jarvis-samples/13519af9-186d-4406-8112-959afcc58158.jsonl";
    println!("📋 测试 thinking 解析:");
    if let Ok(Some(result)) = ClaudeAdapter::parse_session_from_path(thinking_file) {
        let thinking_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| m.content.full.contains("[Thinking]"))
            .take(1)
            .collect();
        if !thinking_msgs.is_empty() {
            println!("  ✅ Thinking 解析成功:");
            let preview: String = thinking_msgs[0].content.full.chars().take(200).collect();
            println!("    {}", preview);
        } else {
            println!("  ❌ 未找到 Thinking 内容");
        }
    }
    println!();

    // 测试 Claude Code
    let home = std::env::var("HOME").unwrap();
    let claude_projects = PathBuf::from(&home).join(".claude/projects");

    println!("📂 Claude projects 路径: {:?}", claude_projects);

    let claude = ClaudeAdapter::with_path(claude_projects);
    match claude.list_sessions() {
        Ok(sessions) => {
            println!("✅ 找到 {} 个 Claude 会话\n", sessions.len());

            // 显示最近 3 个会话
            for (i, session) in sessions.iter().take(3).enumerate() {
                println!("--- 会话 {} ---", i + 1);
                println!("  ID: {}", session.id);
                println!("  项目: {}", session.project_path);
                println!("  文件: {:?}", session.session_path);

                // 尝试解析
                if let Ok(Some(result)) = claude.parse_session(session) {
                    println!("  消息数: {}", result.messages.len());
                    println!("  创建时间: {:?}", result.created_at);
                    println!("  CWD: {:?}", result.cwd);

                    // 显示包含 tool_use 的消息
                    let tool_msgs: Vec<_> = result
                        .messages
                        .iter()
                        .filter(|m| m.content.full.contains("[Tool:"))
                        .take(3)
                        .collect();

                    if !tool_msgs.is_empty() {
                        println!("  📦 找到 {} 条包含 tool_use 的消息:", tool_msgs.len());
                        for msg in tool_msgs {
                            let preview: String = msg.content.full.chars().take(150).collect();
                            println!("    {}", preview);
                        }
                    } else {
                        // 显示前 2 条消息
                        for (j, msg) in result.messages.iter().take(2).enumerate() {
                            let content_preview: String =
                                msg.content.full.chars().take(60).collect();
                            let suffix = if msg.content.full.chars().count() > 60 {
                                "..."
                            } else {
                                ""
                            };
                            println!(
                                "  [{}] {}: {}{}",
                                j + 1,
                                msg.message_type,
                                content_preview,
                                suffix
                            );
                        }
                    }
                }
                println!();
            }
        }
        Err(e) => {
            println!("❌ Claude 解析失败: {}", e);
        }
    }

    // 测试 Codex CLI
    let codex_path = PathBuf::from(&home).join(".codex");
    println!("\n📂 Codex 路径: {:?}", codex_path);

    let codex = CodexAdapter::with_path(codex_path);
    match codex.list_sessions() {
        Ok(sessions) => {
            println!("✅ 找到 {} 个 Codex 会话", sessions.len());

            if let Some(session) = sessions.first() {
                println!("  首个会话 ID: {}", session.id);
                if let Ok(Some(result)) = codex.parse_session(session) {
                    println!("  消息数: {}", result.messages.len());
                }
            }
        }
        Err(e) => {
            println!("⚠️  Codex 解析: {} (可能未安装)", e);
        }
    }

    println!("\n=== 测试完成 ===");
}
