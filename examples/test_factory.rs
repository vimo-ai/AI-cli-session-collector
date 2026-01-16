//! 验证适配器自注册工厂函数

use ai_cli_session_collector::adapter::{
    all_adapters, all_extensions, all_watch_configs, adapter_for_path, ConversationAdapter,
};
use std::path::Path;

fn main() {
    println!("=== 验证适配器自注册模式 ===\n");

    // 1. 验证 all_adapters()
    println!("1️⃣ all_adapters():");
    let adapters = all_adapters();
    println!("   注册了 {} 个适配器:", adapters.len());
    for adapter in &adapters {
        let meta = adapter.meta();
        println!(
            "   - {} (source={}, path={}, recursive={})",
            meta.name,
            meta.source,
            adapter.data_path().display(),
            meta.recursive
        );
    }
    println!();

    // 2. 验证 all_watch_configs()
    println!("2️⃣ all_watch_configs():");
    let configs = all_watch_configs();
    println!("   生成了 {} 个监听配置:", configs.len());
    for config in &configs {
        println!(
            "   - {} (recursive={}, ext={:?})",
            config.path.display(),
            config.recursive,
            config.extensions
        );
    }
    println!();

    // 3. 验证 all_extensions()
    println!("3️⃣ all_extensions():");
    let extensions = all_extensions();
    println!("   支持的扩展名: {:?}", extensions);
    println!();

    // 4. 验证 adapter_for_path()
    println!("4️⃣ adapter_for_path() 路径匹配测试:");
    let home = std::env::var("HOME").unwrap();
    let test_paths = [
        format!("{}/.claude/projects/foo/session.jsonl", home),
        format!("{}/.codex/history/session.yaml", home),
        format!("{}/.local/share/opencode/sessions/session.json", home),
        format!("{}/random/file.txt", home),
    ];
    for path in &test_paths {
        let p = Path::new(path);
        match adapter_for_path(p) {
            Some(adapter) => {
                println!("   ✅ {} -> {}", path, adapter.meta().name);
            }
            None => {
                println!("   ❌ {} -> 无匹配", path);
            }
        }
    }
    println!();

    println!("=== 验证完成 ===");
}
