//! F2FS CRC32 计算。
//!
//! F2FS 使用标准 CRC32 (poly 0xEDB88320), 以 `F2FS_MAGIC` 为初始值,
//! 最终结果不取反。

use crate::f2fs::consts::F2FS_MAGIC;

/// F2FS CRC32: 以 magic 为初值, 不对结果取反。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = F2FS_MAGIC;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

/// inode 校验和: 先 crc(ino 字节), 再 crc(inode 数据, 跳过校验和字段)。
pub fn inode_checksum(ino: u32, inode_data: &[u8]) -> u32 {
    let mut crc = F2FS_MAGIC;
    for byte in ino.to_le_bytes() {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    for (i, &byte) in inode_data.iter().enumerate() {
        // 跳过 i_inode_checksum (偏移 368..372)
        if (368..372).contains(&i) {
            continue;
        }
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}
