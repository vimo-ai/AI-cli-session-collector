//! JSONL 增量读取模块
//!
//! 提供 JSONL 文件的增量读取能力，避免每次文件变化都解析整个文件。
//!
//! ## 设计目标
//!
//! - 支持大文件（75MB+）的高效增量读取
//! - 自动检测文件截断和替换
//! - 正确处理残行（不以换行符结尾的行）
//! - 跳过无效 UTF-8 内容
//!
//! ## 使用示例
//!
//! ```ignore
//! use ai_cli_session_collector::incremental::{JsonlIncrementalReader, ReaderState};
//!
//! let reader = JsonlIncrementalReader::new();
//!
//! // 第一次读取（从头开始）
//! let (lines, stats, state) = reader.read_full(path)?;
//!
//! // 保存 state.offset 和 state.file_id 到数据库...
//!
//! // 后续增量读取
//! let saved_state = ReaderState::from_saved(offset, file_id);
//! let (new_lines, stats, new_state) = reader.read_incremental(path, Some(saved_state))?;
//! ```

mod reader;
mod state;

pub use reader::JsonlIncrementalReader;
pub use state::{FileIdentity, ReadStats, ReaderState};
