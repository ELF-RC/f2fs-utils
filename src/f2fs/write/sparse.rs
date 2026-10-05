//! Android sparse image 编码器。
//!
//! 将 raw 镜像转换为 Android sparse 格式 (AOSP `system/core/libsparse`):
//! 全零块合并为 DONT_CARE, 非零块合并为 RAW。流式处理, 支持大镜像。
//!
//! 格式: 28B sparse header + 若干 (12B chunk header + data)。

use crate::f2fs::consts::F2FS_BLKSIZE;
use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const SPARSE_MAGIC: u32 = 0xED26_FF3A;
const SPARSE_MAJOR_VERSION: u16 = 1;
const SPARSE_MINOR_VERSION: u16 = 0;
const SPARSE_HEADER_SIZE: u16 = 28;
const CHUNK_HEADER_SIZE: u16 = 12;

const CHUNK_TYPE_RAW: u16 = 0xCAC1;
const CHUNK_TYPE_DONT_CARE: u16 = 0xCAC3;

/// 单个 RAW chunk 的最大块数 (避免单 chunk 过大)。
const MAX_RAW_CHUNK_BLKS: u32 = 2047;

/// 将路径处的 raw 镜像原地转换为 Android sparse 镜像。
///
/// 读取 raw 文件, 写 sparse 到 `<path>.sparse.tmp`, 完成后原子替换原文件。
pub fn convert_file(path: &Path) -> Result<()> {
    let file_size = fs::metadata(path)
        .with_context(|| format!("stat raw image {}", path.display()))?
        .len();
    if file_size % F2FS_BLKSIZE as u64 != 0 {
        anyhow::bail!("image size not block-aligned: {file_size}");
    }
    let total_blks = (file_size / F2FS_BLKSIZE as u64) as u32;

    let tmp_path = path.with_extension("sparse.tmp");
    let mut reader =
        File::open(path).with_context(|| format!("open raw image {}", path.display()))?;
    let mut writer = File::create(&tmp_path)
        .with_context(|| format!("create sparse output {}", tmp_path.display()))?;

    // 占位 sparse header (total_chunks 稍后回填)
    let header = build_sparse_header(total_blks, 0);
    writer.write_all(&header)?;

    let mut buf = vec![0u8; F2FS_BLKSIZE];
    let mut total_chunks: u32 = 0;
    // 当前 chunk 状态
    let mut run_type: RunType = RunType::None;
    let mut run_len: u32 = 0;

    for blk_idx in 0..total_blks {
        reader.read_exact(&mut buf)?;
        let is_zero = buf.iter().all(|&b| b == 0);

        let cur_type = if is_zero {
            RunType::DontCare
        } else {
            RunType::Raw
        };
        if cur_type == run_type {
            run_len += 1;
            // RAW chunk 达到上限时提前 flush
            if run_type == RunType::Raw && run_len >= MAX_RAW_CHUNK_BLKS {
                // run 覆盖 [blk_idx+1-run_len, blk_idx]; resume = blk_idx+1 (下一轮读)
                flush_chunk(
                    &mut writer,
                    run_type,
                    run_len,
                    &mut reader,
                    blk_idx + 1 - run_len,
                    blk_idx + 1,
                )?;
                total_chunks += 1;
                run_type = RunType::None;
                run_len = 0;
            }
        } else {
            if run_len > 0 {
                // 旧 run 覆盖 [blk_idx-run_len, blk_idx-1]; blk_idx 是新 run 首块
                flush_chunk(
                    &mut writer,
                    run_type,
                    run_len,
                    &mut reader,
                    blk_idx - run_len,
                    blk_idx + 1,
                )?;
                total_chunks += 1;
            }
            run_type = cur_type;
            run_len = 1;
        }
    }
    // flush 末尾 chunk: run 覆盖 [total_blks-run_len, total_blks-1]; 无下一轮
    if run_len > 0 {
        flush_chunk(
            &mut writer,
            run_type,
            run_len,
            &mut reader,
            total_blks - run_len,
            total_blks,
        )?;
        total_chunks += 1;
    }

    // 回填 total_chunks
    writer.seek(SeekFrom::Start(20))?;
    writer.write_all(&total_chunks.to_le_bytes())?;
    writer.flush()?;
    drop(writer);

    fs::rename(&tmp_path, path)
        .with_context(|| format!("rename sparse over {}", path.display()))?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunType {
    None,
    Raw,
    DontCare,
}

/// flush 一个 chunk; RAW 需从 start_blk 重读 run_len 个块写入, 之后 reader 定位到 resume_blk。
/// start_blk / resume_blk 均为块号 (run 末块+1 / 下一轮首块)。
fn flush_chunk(
    writer: &mut File,
    run_type: RunType,
    run_len: u32,
    reader: &mut File,
    start_blk: u32,
    resume_blk: u32,
) -> Result<()> {
    match run_type {
        RunType::DontCare => {
            let chunk = build_chunk_header(CHUNK_TYPE_DONT_CARE, run_len, 0);
            writer.write_all(&chunk)?;
        }
        RunType::Raw => {
            let chunk =
                build_chunk_header(CHUNK_TYPE_RAW, run_len, run_len as usize * F2FS_BLKSIZE);
            writer.write_all(&chunk)?;
            // 从 run 起点重读 run_len 个块的数据
            reader.seek(SeekFrom::Start(u64::from(start_blk) * F2FS_BLKSIZE as u64))?;
            let mut buf = vec![0u8; F2FS_BLKSIZE];
            for _ in 0..run_len {
                reader.read_exact(&mut buf)?;
                writer.write_all(&buf)?;
            }
            // 恢复 reader 到下一轮读取位置
            reader.seek(SeekFrom::Start(u64::from(resume_blk) * F2FS_BLKSIZE as u64))?;
        }
        RunType::None => {}
    }
    Ok(())
}

fn build_sparse_header(total_blks: u32, total_chunks: u32) -> [u8; 28] {
    let mut buf = [0u8; 28];
    buf[..4].copy_from_slice(&SPARSE_MAGIC.to_le_bytes());
    buf[4..6].copy_from_slice(&SPARSE_MAJOR_VERSION.to_le_bytes());
    buf[6..8].copy_from_slice(&SPARSE_MINOR_VERSION.to_le_bytes());
    buf[8..10].copy_from_slice(&SPARSE_HEADER_SIZE.to_le_bytes());
    buf[10..12].copy_from_slice(&CHUNK_HEADER_SIZE.to_le_bytes());
    buf[12..16].copy_from_slice(&(F2FS_BLKSIZE as u32).to_le_bytes());
    buf[16..20].copy_from_slice(&total_blks.to_le_bytes());
    buf[20..24].copy_from_slice(&total_chunks.to_le_bytes());
    buf[24..28].copy_from_slice(&0u32.to_le_bytes()); // checksum 不校验
    buf
}

fn build_chunk_header(chunk_type: u16, chunk_sz: u32, data_bytes: usize) -> [u8; 12] {
    let mut buf = [0u8; 12];
    buf[..2].copy_from_slice(&chunk_type.to_le_bytes());
    // reserved (2 字节) 留零
    buf[4..8].copy_from_slice(&chunk_sz.to_le_bytes());
    let total_sz = CHUNK_HEADER_SIZE as usize + data_bytes;
    buf[8..12].copy_from_slice(&(total_sz as u32).to_le_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparse_header_layout() {
        let h = build_sparse_header(100, 3);
        assert_eq!(u32::from_le_bytes([h[0], h[1], h[2], h[3]]), SPARSE_MAGIC);
        assert_eq!(u32::from_le_bytes([h[16], h[17], h[18], h[19]]), 100);
        assert_eq!(u32::from_le_bytes([h[20], h[21], h[22], h[23]]), 3);
    }

    #[test]
    fn test_chunk_header_raw() {
        let c = build_chunk_header(CHUNK_TYPE_RAW, 5, 5 * F2FS_BLKSIZE);
        assert_eq!(u16::from_le_bytes([c[0], c[1]]), CHUNK_TYPE_RAW);
        assert_eq!(u32::from_le_bytes([c[4], c[5], c[6], c[7]]), 5);
        let expected = u32::from(CHUNK_HEADER_SIZE) + (5 * F2FS_BLKSIZE) as u32;
        assert_eq!(u32::from_le_bytes([c[8], c[9], c[10], c[11]]), expected);
    }
}
