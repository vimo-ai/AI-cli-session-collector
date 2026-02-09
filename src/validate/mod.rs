//! 数据结构合规性验证器
//!
//! 验证 AI CLI 会话数据是否符合已知规范，未知结构立即报出。
//! 验证层级: L0 Field → L1 Line → L2 Block → L3 Turn → L4 Session → L5 Project

pub mod claude;
pub mod framework;
pub mod report;

use framework::ValidationResult;
use std::path::PathBuf;

/// 执行 Claude Code 数据验证
pub fn run_claude_validation(
    projects_path: Option<PathBuf>,
    single_session: Option<PathBuf>,
    max_level: u8,
) -> ValidationResult {
    let path = projects_path.unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".claude/projects")
    });

    let mut validator = claude::ClaudeValidator::new(path, max_level);
    if let Some(session) = single_session {
        validator = validator.with_single_session(session);
    }
    validator.run()
}
