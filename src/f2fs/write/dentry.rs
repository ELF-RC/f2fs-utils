//! 目录块构建器: 常规 dentry block、inline dentry、TEA 哈希。

use crate::f2fs::consts::{F2FS_BLKSIZE, F2FS_NAME_LEN, F2FS_SLOT_LEN};
use crate::f2fs::write::consts::{
    DENTRY_BITMAP_SIZE, DENTRY_RESERVED_SIZE, F2FS_DIR_ENTRY_SIZE, INLINE_DENTRY_BITMAP_SIZE,
    INLINE_RESERVED_SIZE, NR_DENTRY_IN_BLOCK_W, NR_INLINE_DENTRY_W,
};
use crate::f2fs::write::types::{DirEntryRaw, FileType};

/// 目录项信息 (构建期)。
#[derive(Debug, Clone)]
pub struct DentryInfo {
    pub name: Vec<u8>,
    pub ino: u32,
    pub file_type: FileType,
}

impl DentryInfo {
    pub fn new(name: &[u8], ino: u32, file_type: FileType) -> Self {
        Self {
            name: name.to_vec(),
            ino,
            file_type,
        }
    }

    pub fn slots_needed(&self) -> usize {
        self.name.len().div_ceil(F2FS_SLOT_LEN).max(1)
    }
}

/// 常规 dentry block 构建器 (4KiB)。
#[derive(Debug, Default)]
pub struct DentryBlockBuilder {
    entries: Vec<DentryInfo>,
    used_slots: usize,
}

impl DentryBlockBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    fn can_add(&self, entry: &DentryInfo) -> bool {
        self.used_slots + entry.slots_needed() <= NR_DENTRY_IN_BLOCK_W
    }

    pub fn add_entry(&mut self, entry: DentryInfo) -> bool {
        if !self.can_add(&entry) {
            return false;
        }
        self.used_slots += entry.slots_needed();
        self.entries.push(entry);
        true
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn build(&self) -> [u8; F2FS_BLKSIZE] {
        let mut buf = [0u8; F2FS_BLKSIZE];
        let bitmap_off = 0;
        let dentry_off = DENTRY_BITMAP_SIZE + DENTRY_RESERVED_SIZE;
        let filename_off = dentry_off + NR_DENTRY_IN_BLOCK_W * F2FS_DIR_ENTRY_SIZE;

        let mut slot_idx = 0;
        let mut name_offset = 0;
        for entry in &self.entries {
            let slots = entry.slots_needed();
            let hash = dentry_hash(&entry.name);

            for i in 0..slots {
                let bit = slot_idx + i;
                buf[bitmap_off + bit / 8] |= 1 << (bit % 8);
            }

            let raw = DirEntryRaw {
                hash_code: hash,
                ino: entry.ino,
                name_len: entry.name.len() as u16,
                file_type: entry.file_type.as_u8(),
            };
            let entry_off = dentry_off + slot_idx * F2FS_DIR_ENTRY_SIZE;
            buf[entry_off..entry_off + F2FS_DIR_ENTRY_SIZE].copy_from_slice(&raw.to_bytes());

            let name_start = filename_off + name_offset;
            let name_end = name_start + entry.name.len();
            if name_end <= F2FS_BLKSIZE {
                buf[name_start..name_end].copy_from_slice(&entry.name);
            }

            slot_idx += slots;
            name_offset += slots * F2FS_SLOT_LEN;
        }
        buf
    }
}

// ===== TEA 哈希 (F2FS 目录项哈希算法) =====

const F2FS_HASH_COL_BIT: u64 = 1u64 << 63;
const DELTA: u32 = 0x9E37_79B9;

fn tea_transform(buf: &mut [u32; 4], input: &[u32; 4]) {
    let mut sum: u32 = 0;
    let mut b0 = buf[0];
    let mut b1 = buf[1];
    let (a, b, c, d) = (input[0], input[1], input[2], input[3]);
    for _ in 0..16 {
        sum = sum.wrapping_add(DELTA);
        b0 = b0.wrapping_add(
            ((b1 << 4).wrapping_add(a)) ^ (b1.wrapping_add(sum)) ^ ((b1 >> 5).wrapping_add(b)),
        );
        b1 = b1.wrapping_add(
            ((b0 << 4).wrapping_add(c)) ^ (b0.wrapping_add(sum)) ^ ((b0 >> 5).wrapping_add(d)),
        );
    }
    buf[0] = buf[0].wrapping_add(b0);
    buf[1] = buf[1].wrapping_add(b1);
}

fn str2hashbuf(msg: &[u8], len: usize, buf: &mut [u32; 4]) {
    let pad = (len as u32) | ((len as u32) << 8);
    let pad = pad | (pad << 16);

    let mut val = pad;
    let actual = len.min(16);
    for (i, &byte) in msg.iter().take(actual).enumerate() {
        if i % 4 == 0 {
            val = pad;
        }
        val = (u32::from(byte)).wrapping_add(val << 8);
        if i % 4 == 3 {
            buf[i / 4] = val;
            val = pad;
        }
    }
    if !actual.is_multiple_of(4) {
        buf[actual / 4] = val;
    }
    let filled = actual.div_ceil(4);
    for item in buf.iter_mut().skip(filled) {
        *item = pad;
    }
}

/// 计算目录项哈希 (TEA)。
pub fn dentry_hash(name: &[u8]) -> u32 {
    if name.is_empty() || name == b"." || name == b".." {
        return 0;
    }
    let mut buf: [u32; 4] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476];
    let mut p = name;
    let mut len = name.len();
    loop {
        let mut input: [u32; 4] = [0; 4];
        str2hashbuf(p, len, &mut input);
        tea_transform(&mut buf, &input);
        if len <= 16 {
            break;
        }
        p = &p[16..];
        len -= 16;
    }
    ((u64::from(buf[0])) & !F2FS_HASH_COL_BIT) as u32
}

/// inline 目录构建器 (嵌入 inode 块内)。
#[derive(Debug, Default)]
pub struct InlineDentryBuilder {
    entries: Vec<DentryInfo>,
    used_slots: usize,
}

impl InlineDentryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    fn can_add(&self, entry: &DentryInfo) -> bool {
        self.used_slots + entry.slots_needed() <= NR_INLINE_DENTRY_W
    }

    pub fn add_entry(&mut self, entry: DentryInfo) -> bool {
        if !self.can_add(&entry) {
            return false;
        }
        self.used_slots += entry.slots_needed();
        self.entries.push(entry);
        true
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn build(&self) -> Vec<u8> {
        let total = INLINE_DENTRY_BITMAP_SIZE
            + INLINE_RESERVED_SIZE
            + NR_INLINE_DENTRY_W * F2FS_DIR_ENTRY_SIZE
            + NR_INLINE_DENTRY_W * F2FS_SLOT_LEN;
        let mut buf = vec![0u8; total];
        let bitmap_off = 0;
        let dentry_off = INLINE_DENTRY_BITMAP_SIZE + INLINE_RESERVED_SIZE;
        let filename_off = dentry_off + NR_INLINE_DENTRY_W * F2FS_DIR_ENTRY_SIZE;

        let mut slot_idx = 0;
        let mut name_offset = 0;
        for entry in &self.entries {
            let slots = entry.slots_needed();
            let hash = dentry_hash(&entry.name);
            for i in 0..slots {
                let bit = slot_idx + i;
                if bit / 8 < INLINE_DENTRY_BITMAP_SIZE {
                    buf[bitmap_off + bit / 8] |= 1 << (bit % 8);
                }
            }
            let raw = DirEntryRaw {
                hash_code: hash,
                ino: entry.ino,
                name_len: entry.name.len().min(F2FS_NAME_LEN) as u16,
                file_type: entry.file_type.as_u8(),
            };
            let entry_off = dentry_off + slot_idx * F2FS_DIR_ENTRY_SIZE;
            if entry_off + F2FS_DIR_ENTRY_SIZE <= buf.len() {
                buf[entry_off..entry_off + F2FS_DIR_ENTRY_SIZE].copy_from_slice(&raw.to_bytes());
            }
            let name_start = filename_off + name_offset;
            let name_end = name_start + entry.name.len();
            if name_end <= buf.len() {
                buf[name_start..name_end].copy_from_slice(&entry.name);
            }
            slot_idx += slots;
            name_offset += slots * F2FS_SLOT_LEN;
        }
        buf
    }
}
