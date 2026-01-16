//! 测试 OpenCode 适配器

use ai_cli_session_collector::adapter::{OpenCodeAdapter, ConversationAdapter};

fn main() {
    tracing_subscriber::fmt::init();

    println!("=== OpenCode 适配器测试 ===\n");

    let adapter = OpenCodeAdapter::new();

    println!("数据路径: {:?}", adapter.data_path());
    println!();

    match adapter.list_sessions() {
        Ok(sessions) => {
            println!("✅ 找到 {} 个 OpenCode 会话\n", sessions.len());

            for (i, session) in sessions.iter().take(5).enumerate() {
                println!("--- 会话 {} ---", i + 1);
                println!("  ID: {}", session.id);
                println!("  项目: {}", session.project_path);
                println!("  文件: {:?}", session.session_path);
                println!("  创建时间: {:?}", session.created_at);

                // 尝试解析
                if let Ok(Some(result)) = adapter.parse_session(session) {
                    println!("  消息数: {}", result.messages.len());

                    // 显示前 2 条消息
                    for (j, msg) in result.messages.iter().take(2).enumerate() {
                        let preview: String = msg.content.text.chars().take(80).collect();
                        println!("    [{}] {}: {}...", j + 1, msg.message_type, preview);
                    }
                }
                println!();
            }
        }
        Err(e) => {
            println!("❌ OpenCode 解析失败: {}", e);
        }
    }

    println!("=== 测试完成 ===");
}
