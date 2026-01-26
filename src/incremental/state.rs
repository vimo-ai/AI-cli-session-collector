//! 增量读取状态管理
//!
//! 提供文件标识和读取状态的数据结构，用于支持 JSONL 文件的增量读取。

/// 文件标识信息
///
/// 用于检测文件是否被截断或替换
#[derive(Debug, Clone, Default)]
pub struct FileIdentity {
    /// 文件 inode（用于检测文件替换）
    pub inode: u64,
    /// 文件修改时间（毫秒时间戳）
    pub mtime: u64,
    /// 文件大小（字节）
    pub size: u64,
}

impl FileIdentity {
    /// 创建新的文件标识
    pub fn new(inode: u64, mtime: u64, size: u64) -> Self {
        Self { inode, mtime, size }
    }

    /// 检查是否需要重置（文件被截断或替换）
    ///
    /// 返回 true 表示需要从头开始读取
    pub fn needs_reset(&self, current: &FileIdentity) -> bool {
        // inode 变化 = 文件被替换
        if self.inode != current.inode {
            return true;
        }
        // 文件变小 = 文件被截断
        if current.size < self.size {
            return true;
        }
        false
    }
}

/// 读取状态
///
/// 保存增量读取的进度信息
#[derive(Debug, Clone, Default)]
pub struct ReaderState {
    /// 当前字节偏移（已读取到的位置）
    pub offset: u64,
    /// 残行缓冲（上次读取的不完整行）
    pub partial_line: Vec<u8>,
    /// 文件标识（用于检测文件变化）
    pub file_id: Option<FileIdentity>,
}

impl ReaderState {
    /// 创建新的读取状态
    pub fn new() -> Self {
        Self::default()
    }

    /// 从保存的状态恢复
    pub fn from_saved(offset: u64, file_id: FileIdentity) -> Self {
        Self {
            offset,
            partial_line: Vec::new(),
            file_id: Some(file_id),
        }
    }

    /// 重置状态（从头开始）
    pub fn reset(&mut self) {
        self.offset = 0;
        self.partial_line.clear();
        self.file_id = None;
    }

    /// 更新偏移量
    pub fn update_offset(&mut self, new_offset: u64) {
        self.offset = new_offset;
    }

    /// 设置残行缓冲
    pub fn set_partial_line(&mut self, data: Vec<u8>) {
        self.partial_line = data;
    }

    /// 获取并清空残行缓冲
    pub fn take_partial_line(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.partial_line)
    }
}

/// 读取统计信息
#[derive(Debug, Clone, Default)]
pub struct ReadStats {
    /// 读取的行数
    pub lines_read: usize,
    /// 读取的字节数
    pub bytes_read: u64,
    /// 是否发生了重置（文件被截断或替换）
    pub was_reset: bool,
    /// 跳过的无效行数
    pub invalid_lines: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_identity_needs_reset() {
        let old = FileIdentity::new(123, 1000, 500);

        // 相同文件，大小增加 -> 不需要重置
        let current = FileIdentity::new(123, 1001, 600);
        assert!(!old.needs_reset(&current));

        // 相同 inode，大小减小 -> 需要重置（截断）
        let truncated = FileIdentity::new(123, 1002, 400);
        assert!(old.needs_reset(&truncated));

        // 不同 inode -> 需要重置（替换）
        let replaced = FileIdentity::new(456, 1000, 500);
        assert!(old.needs_reset(&replaced));
    }

    #[test]
    fn test_reader_state() {
        let mut state = ReaderState::new();
        assert_eq!(state.offset, 0);
        assert!(state.partial_line.is_empty());

        state.update_offset(100);
        assert_eq!(state.offset, 100);

        state.set_partial_line(b"partial".to_vec());
        assert_eq!(state.partial_line, b"partial");

        let partial = state.take_partial_line();
        assert_eq!(partial, b"partial");
        assert!(state.partial_line.is_empty());

        state.reset();
        assert_eq!(state.offset, 0);
    }
}
