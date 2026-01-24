//! 测试 cwd 提取和路径计算

use ai_cli_session_collector::ClaudeAdapter;
use ai_cli_session_collector::ConversationAdapter;
use std::path::PathBuf;

fn main() {
    let projects_path = PathBuf::from(std::env::var("HOME").unwrap()).join(".claude/projects");

    println!("=== 测试 cwd 提取 ===\n");

    let adapter = ClaudeAdapter::with_path(projects_path.clone());

    // 列出所有会话
    let sessions = adapter.list_sessions().expect("列出会话失败");
    println!("找到 {} 个会话\n", sessions.len());

    // 显示前 5 个会话的信息
    for session in sessions.iter().take(5) {
        println!("Session ID: {}", session.id);
        println!("  project_path: {}", session.project_path);
        println!("  encoded_dir_name: {:?}", session.encoded_dir_name);
        println!("  session_path: {:?}", session.session_path);
        println!("  cwd: {:?}", session.cwd);
        println!();
    }

    // 验证：project_path 不应该是编码后的形式
    let mut errors = 0;
    let mut empty_path_count = 0;
    for session in &sessions {
        if session.project_path.is_empty() {
            empty_path_count += 1;
            continue;
        }
        if session.project_path.starts_with("-") && session.project_path.contains("-Users-") {
            println!(
                "❌ 错误：project_path 仍然是编码形式: {}",
                session.project_path
            );
            errors += 1;
        }
    }

    println!("\n=== 统计 ===");
    println!("总会话数: {}", sessions.len());
    println!("空 project_path（无 user 消息）: {}", empty_path_count);
    println!("正常会话: {}", sessions.len() - empty_path_count);

    if errors == 0 {
        println!("✅ 所有有效会话的 project_path 都是正确的路径格式");
    } else {
        println!("\n❌ 发现 {} 个错误", errors);
    }

    // 测试路径计算
    println!("\n=== 测试路径计算 ===\n");
    if let Some(session) = sessions.iter().find(|s| !s.project_path.is_empty()) {
        if let (Some(encoded), Some(session_path)) =
            (&session.encoded_dir_name, &session.session_path)
        {
            let computed = projects_path
                .join(encoded)
                .join(format!("{}.jsonl", session.id));
            let computed_str = computed.to_string_lossy().to_string();

            if computed_str == *session_path {
                println!("✅ 路径计算正确:");
                println!("   computed:      {}", computed_str);
                println!("   session_path:  {}", session_path);
            } else {
                println!("❌ 路径计算不匹配:");
                println!("   computed:      {}", computed_str);
                println!("   session_path:  {}", session_path);
            }
        }
    }
}
