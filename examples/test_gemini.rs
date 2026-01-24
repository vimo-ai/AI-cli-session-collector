//! Gemini CLI adapter 测试示例

use ai_cli_session_collector::adapter::ConversationAdapter;
use ai_cli_session_collector::adapter::GeminiAdapter;

fn main() {
    // 初始化日志
    tracing_subscriber::fmt::init();

    let adapter = GeminiAdapter::new();

    println!("=== Gemini CLI Adapter 测试 ===");
    println!("数据路径: {:?}", adapter.data_path());
    println!();

    // 列出所有会话
    match adapter.list_sessions() {
        Ok(sessions) => {
            println!("找到 {} 个会话", sessions.len());
            println!();

            for (i, session) in sessions.iter().take(5).enumerate() {
                println!("--- 会话 {} ---", i + 1);
                println!("  ID: {}", session.id);
                println!("  项目: {:?}", session.project_name);
                println!("  消息数: {:?}", session.message_count);
                println!("  创建时间: {:?}", session.created_at);
                println!("  更新时间: {:?}", session.updated_at);
                println!("  模型: {:?}", session.model);
                println!();

                // 解析会话内容
                if let Ok(Some(result)) = adapter.parse_session(session) {
                    println!("  解析后消息数: {}", result.messages.len());
                    for (j, msg) in result.messages.iter().take(3).enumerate() {
                        println!(
                            "    消息 {}: [{:?}] {}...",
                            j + 1,
                            msg.message_type,
                            msg.content.text.chars().take(50).collect::<String>()
                        );
                    }
                    if result.messages.len() > 3 {
                        println!("    ... 还有 {} 条消息", result.messages.len() - 3);
                    }
                }
                println!();
            }

            if sessions.len() > 5 {
                println!("... 还有 {} 个会话未显示", sessions.len() - 5);
            }
        }
        Err(e) => {
            eprintln!("列出会话失败: {}", e);
        }
    }
}
