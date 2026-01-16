use ai_cli_session_collector::adapter::{CodexAdapter, ConversationAdapter};
use std::path::PathBuf;

fn main() {
    // 初始化日志
    tracing_subscriber::fmt::init();

    let codex_root = PathBuf::from(std::env::var("HOME").unwrap()).join(".codex");
    let adapter = CodexAdapter::with_path(codex_root);

    println!("=== Codex Adapter 真实数据测试 ===\n");

    // 1. 列出会话
    println!("📋 列出会话...");
    let sessions = adapter.list_sessions().expect("list_sessions failed");
    println!("找到 {} 个会话\n", sessions.len());

    // 显示前 5 个会话
    for (i, session) in sessions.iter().take(5).enumerate() {
        println!("Session #{}", i + 1);
        println!("  ID: {}", session.id);
        println!("  项目: {}", session.project_name.as_deref().unwrap_or("unknown"));
        println!("  路径: {}", session.project_path);
        println!("  cwd: {:?}", session.cwd);
        println!("  session_path: {:?}", session.session_path);
        println!("  created_at: {:?}", session.created_at);
        println!();
    }

    // 2. 解析第一个有 session_path 的会话
    if let Some(session) = sessions.iter().find(|s| s.session_path.is_some()) {
        println!("📖 解析会话: {}", session.id);
        match adapter.parse_session(session) {
            Ok(Some(result)) => {
                println!("  消息数: {}", result.messages.len());
                println!("  cwd: {:?}", result.cwd);
                println!("  model: {:?}", result.model);
                println!("  created_at: {:?}", result.created_at);
                println!("  updated_at: {:?}", result.updated_at);

                // 显示前 3 条消息
                println!("\n  前 3 条消息:");
                for (i, msg) in result.messages.iter().take(3).enumerate() {
                    println!("    [{}] {} - {}: {}...",
                        i + 1,
                        msg.message_type,
                        msg.uuid,
                        &msg.content.text.chars().take(50).collect::<String>()
                    );
                }
            }
            Ok(None) => println!("  会话为空"),
            Err(e) => println!("  解析失败: {}", e),
        }
    } else {
        println!("没有找到可解析的会话");
    }

    println!("\n✅ 测试完成");
}
