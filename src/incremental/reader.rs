//! JSONL 增量读取器
//!
//! 实现 JSONL 文件的增量读取，避免每次文件变化都解析整个文件。

use super::state::{FileIdentity, ReadStats, ReaderState};
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// JSONL 增量读取器
///
/// 支持从上次读取位置继续读取 JSONL 文件，处理：
/// - 文件截断检测
/// - 文件替换检测
/// - 残行处理
/// - 无效 UTF-8 跳过
pub struct JsonlIncrementalReader {
    /// 读取缓冲区大小
    buffer_size: usize,
}

impl Default for JsonlIncrementalReader {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonlIncrementalReader {
    /// 创建新的增量读取器
    pub fn new() -> Self {
        Self {
            buffer_size: 64 * 1024, // 64KB
        }
    }

    /// 设置缓冲区大小
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// 获取文件标识
    fn get_file_identity(path: &Path) -> Result<FileIdentity> {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("无法获取文件元数据: {:?}", path))?;

        // Unix: 使用 inode 检测文件替换
        // Windows: file_index 是 unstable feature，用 0 代替（放弃文件替换检测，但文件截断检测仍有效）
        #[cfg(unix)]
        let inode = metadata.ino();
        #[cfg(not(unix))]
        let inode = 0u64;
        let size = metadata.len();
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        Ok(FileIdentity::new(inode, mtime, size))
    }

    /// 增量读取 JSONL 文件
    ///
    /// # Arguments
    /// * `path` - 文件路径
    /// * `state` - 读取状态（如果为 None，从头开始读取）
    ///
    /// # Returns
    /// * `Ok((lines, stats, new_state))` - 新读取的行、统计信息和更新后的状态
    /// * `Err` - 读取失败
    pub fn read_incremental(
        &self,
        path: &Path,
        state: Option<ReaderState>,
    ) -> Result<(Vec<String>, ReadStats, ReaderState)> {
        let mut stats = ReadStats::default();
        let mut state = state.unwrap_or_default();

        // 获取当前文件标识
        let current_id = Self::get_file_identity(path)?;

        // 检查是否需要重置
        if let Some(ref old_id) = state.file_id
            && old_id.needs_reset(&current_id)
        {
            tracing::info!(
                "文件变化检测到重置: {:?} (inode: {} -> {}, size: {} -> {})",
                path,
                old_id.inode,
                current_id.inode,
                old_id.size,
                current_id.size
            );
            state.reset();
            stats.was_reset = true;
        }

        // 如果 offset 超过文件大小，也需要重置
        if state.offset > current_id.size {
            tracing::info!(
                "偏移量超过文件大小，重置: offset={}, size={}",
                state.offset,
                current_id.size
            );
            state.reset();
            stats.was_reset = true;
        }

        // 如果文件没有变化，直接返回空结果
        if state.offset == current_id.size && state.partial_line.is_empty() {
            state.file_id = Some(current_id);
            return Ok((Vec::new(), stats, state));
        }

        // 打开文件并 seek 到上次位置
        let file = File::open(path).with_context(|| format!("无法打开文件: {:?}", path))?;
        let mut reader = BufReader::with_capacity(self.buffer_size, file);

        if state.offset > 0 {
            reader
                .seek(SeekFrom::Start(state.offset))
                .with_context(|| format!("无法 seek 到偏移量: {}", state.offset))?;
        }

        // 读取新增内容
        let mut lines = Vec::new();
        let mut current_offset = state.offset;

        // 处理上次的残行
        let mut pending_prefix = state.take_partial_line();

        loop {
            let mut buf = Vec::new();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => {
                    // EOF
                    if !pending_prefix.is_empty() {
                        // 保存残行到 state
                        state.set_partial_line(pending_prefix);
                    }
                    break;
                }
                Ok(n) => {
                    current_offset += n as u64;
                    stats.bytes_read += n as u64;

                    // 合并残行
                    let full_line = if !pending_prefix.is_empty() {
                        let mut combined = pending_prefix;
                        combined.extend_from_slice(&buf);
                        pending_prefix = Vec::new();
                        combined
                    } else {
                        buf
                    };

                    // 检查是否是完整行
                    if !full_line.ends_with(b"\n") {
                        // 不完整的行，保存为残行
                        pending_prefix = full_line;
                        continue;
                    }

                    // 转换为字符串
                    let line = match String::from_utf8(full_line) {
                        Ok(s) => s.trim_end_matches('\n').trim_end_matches('\r').to_string(),
                        Err(e) => {
                            tracing::debug!("跳过无效 UTF-8 行: {}", e);
                            stats.invalid_lines += 1;
                            continue;
                        }
                    };

                    // 跳过空行
                    if line.trim().is_empty() {
                        continue;
                    }

                    lines.push(line);
                    stats.lines_read += 1;
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("读取文件失败: {}", e));
                }
            }
        }

        // 更新状态
        state.update_offset(current_offset);
        state.file_id = Some(current_id);

        Ok((lines, stats, state))
    }

    /// 从头完整读取文件（不使用增量）
    ///
    /// 用于首次读取或需要重新解析的场景
    pub fn read_full(&self, path: &Path) -> Result<(Vec<String>, ReadStats, ReaderState)> {
        self.read_incremental(path, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_full() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"user","message":"hello"}}"#).unwrap();
        writeln!(file, r#"{{"type":"assistant","message":"hi"}}"#).unwrap();
        file.flush().unwrap();

        let reader = JsonlIncrementalReader::new();
        let (lines, stats, state) = reader.read_full(file.path()).unwrap();

        assert_eq!(lines.len(), 2);
        assert_eq!(stats.lines_read, 2);
        assert!(!stats.was_reset);
        assert!(state.offset > 0);
    }

    #[test]
    fn test_read_incremental() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"user","message":"hello"}}"#).unwrap();
        file.flush().unwrap();

        let reader = JsonlIncrementalReader::new();

        // 第一次读取
        let (lines1, _, state1) = reader.read_full(file.path()).unwrap();
        assert_eq!(lines1.len(), 1);

        // 追加内容
        writeln!(file, r#"{{"type":"assistant","message":"hi"}}"#).unwrap();
        file.flush().unwrap();

        // 增量读取
        let (lines2, stats2, _) = reader.read_incremental(file.path(), Some(state1)).unwrap();
        assert_eq!(lines2.len(), 1);
        assert_eq!(stats2.lines_read, 1);
        assert!(!stats2.was_reset);
    }

    #[test]
    fn test_truncation_detection() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"user","message":"hello"}}"#).unwrap();
        writeln!(file, r#"{{"type":"assistant","message":"world"}}"#).unwrap();
        file.flush().unwrap();

        let reader = JsonlIncrementalReader::new();

        // 第一次读取
        let (_, _, state1) = reader.read_full(file.path()).unwrap();

        // 截断文件
        file.as_file().set_len(10).unwrap();
        file.flush().unwrap();

        // 增量读取应该检测到截断并重置
        let (_, stats2, _) = reader.read_incremental(file.path(), Some(state1)).unwrap();
        assert!(stats2.was_reset);
    }

    #[test]
    fn test_partial_line() {
        let mut file = NamedTempFile::new().unwrap();
        // 写入不带换行符的内容
        write!(file, r#"{{"type":"user","message":"hello"}}"#).unwrap();
        file.flush().unwrap();

        let reader = JsonlIncrementalReader::new();
        let (lines, _, state) = reader.read_full(file.path()).unwrap();

        // 残行应该被保存，不作为完整行返回
        assert!(lines.is_empty());
        assert!(!state.partial_line.is_empty());

        // 追加换行符
        writeln!(file).unwrap();
        file.flush().unwrap();

        // 再次读取，应该能获取完整行
        let (lines2, _, _) = reader.read_incremental(file.path(), Some(state)).unwrap();
        assert_eq!(lines2.len(), 1);
    }

    #[test]
    fn test_empty_file() {
        let file = NamedTempFile::new().unwrap();

        let reader = JsonlIncrementalReader::new();
        let (lines, stats, _) = reader.read_full(file.path()).unwrap();

        assert!(lines.is_empty());
        assert_eq!(stats.lines_read, 0);
    }

    #[test]
    fn test_skip_empty_lines() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"type":"user","message":"hello"}}"#).unwrap();
        writeln!(file).unwrap(); // 空行
        writeln!(file, "   ").unwrap(); // 只有空格的行
        writeln!(file, r#"{{"type":"assistant","message":"hi"}}"#).unwrap();
        file.flush().unwrap();

        let reader = JsonlIncrementalReader::new();
        let (lines, stats, _) = reader.read_full(file.path()).unwrap();

        assert_eq!(lines.len(), 2);
        assert_eq!(stats.lines_read, 2);
    }
}
