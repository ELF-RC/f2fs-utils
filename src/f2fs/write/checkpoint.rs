//! Checkpoint 构建器。

use crate::f2fs::consts::F2FS_BLKSIZE;
use crate::f2fs::write::consts::{
    CP_CHKSUM_OFFSET, CP_COMPACT_SUM_FLAG_W, CP_UMOUNT_FLAG, F2FS_FIRST_INO,
};
use crate::f2fs::write::crc::crc32;

const MAX_ACTIVE_NODE_LOGS: usize = 8;
const MAX_ACTIVE_DATA_LOGS: usize = 8;
const MAX_ACTIVE_LOGS: usize = 16;

#[derive(Debug)]
pub struct CheckpointBuilder {
    checkpoint_ver: u64,
    user_block_count: u64,
    valid_block_count: u64,
    rsvd_segment_count: u32,
    overprov_segment_count: u32,
    free_segment_count: u32,
    cur_node_segno: [u32; MAX_ACTIVE_NODE_LOGS],
    cur_node_blkoff: [u16; MAX_ACTIVE_NODE_LOGS],
    cur_data_segno: [u32; MAX_ACTIVE_DATA_LOGS],
    cur_data_blkoff: [u16; MAX_ACTIVE_DATA_LOGS],
    ckpt_flags: u32,
    cp_pack_total_block_count: u32,
    cp_pack_start_sum: u32,
    valid_node_count: u32,
    valid_inode_count: u32,
    next_free_nid: u32,
    sit_ver_bitmap_bytesize: u32,
    nat_ver_bitmap_bytesize: u32,
    elapsed_time: u64,
    alloc_type: [u8; MAX_ACTIVE_LOGS],
    sit_bitmap: Vec<u8>,
    nat_bitmap: Vec<u8>,
}

impl Default for CheckpointBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl CheckpointBuilder {
    pub fn new() -> Self {
        Self {
            checkpoint_ver: 1,
            user_block_count: 0,
            valid_block_count: 0,
            rsvd_segment_count: 0,
            overprov_segment_count: 0,
            free_segment_count: 0,
            // curseg 数组高位索引 [3..8] 是 LFS/RSS/ATSS/GC 等保留类型,
            // 官方填 NULL_SEGNO(0xffffffff)/0xffff; 若填 0, 内核 build_curseg
            // 把 segno 0 当有效 curseg, 与 SIT 不一致 → build_segment_manager -117。
            cur_node_segno: [u32::MAX; MAX_ACTIVE_NODE_LOGS],
            cur_node_blkoff: [u16::MAX; MAX_ACTIVE_NODE_LOGS],
            cur_data_segno: [u32::MAX; MAX_ACTIVE_DATA_LOGS],
            cur_data_blkoff: [u16::MAX; MAX_ACTIVE_DATA_LOGS],
            ckpt_flags: CP_UMOUNT_FLAG,
            cp_pack_total_block_count: 2,
            cp_pack_start_sum: 1,
            valid_node_count: 0,
            valid_inode_count: 0,
            next_free_nid: F2FS_FIRST_INO,
            sit_ver_bitmap_bytesize: 0,
            nat_ver_bitmap_bytesize: 0,
            elapsed_time: 0,
            alloc_type: [0; MAX_ACTIVE_LOGS],
            sit_bitmap: Vec::new(),
            nat_bitmap: Vec::new(),
        }
    }

    pub fn with_version(mut self, ver: u64) -> Self {
        self.checkpoint_ver = ver;
        self
    }
    pub fn with_user_block_count(mut self, c: u64) -> Self {
        self.user_block_count = c;
        self
    }
    pub fn with_valid_block_count(mut self, c: u64) -> Self {
        self.valid_block_count = c;
        self
    }
    pub fn with_free_segment_count(mut self, c: u32) -> Self {
        self.free_segment_count = c;
        self
    }
    pub fn with_rsvd_segment_count(mut self, c: u32) -> Self {
        self.rsvd_segment_count = c;
        self
    }
    pub fn with_overprov_segment_count(mut self, c: u32) -> Self {
        self.overprov_segment_count = c;
        self
    }
    pub fn with_flags(mut self, f: u32) -> Self {
        self.ckpt_flags = f;
        self
    }
    pub fn with_valid_node_count(mut self, c: u32) -> Self {
        self.valid_node_count = c;
        self
    }
    pub fn with_valid_inode_count(mut self, c: u32) -> Self {
        self.valid_inode_count = c;
        self
    }
    pub fn with_next_free_nid(mut self, n: u32) -> Self {
        self.next_free_nid = n;
        self
    }
    pub fn with_sit_bitmap(mut self, bm: Vec<u8>) -> Self {
        self.sit_ver_bitmap_bytesize = bm.len() as u32;
        self.sit_bitmap = bm;
        self
    }
    pub fn with_nat_bitmap(mut self, bm: Vec<u8>) -> Self {
        self.nat_ver_bitmap_bytesize = bm.len() as u32;
        self.nat_bitmap = bm;
        self
    }
    pub fn with_cp_pack_total_block_count(mut self, c: u32) -> Self {
        self.cp_pack_total_block_count = c;
        self
    }

    pub fn set_cur_node_seg(&mut self, idx: usize, segno: u32, blkoff: u16) {
        if idx < MAX_ACTIVE_NODE_LOGS {
            self.cur_node_segno[idx] = segno;
            self.cur_node_blkoff[idx] = blkoff;
        }
    }
    pub fn set_cur_data_seg(&mut self, idx: usize, segno: u32, blkoff: u16) {
        if idx < MAX_ACTIVE_DATA_LOGS {
            self.cur_data_segno[idx] = segno;
            self.cur_data_blkoff[idx] = blkoff;
        }
    }

    /// 序列化 checkpoint 头部 (4KiB)。
    pub fn build(&self) -> Vec<u8> {
        let mut buf = vec![0u8; F2FS_BLKSIZE];
        buf[..8].copy_from_slice(&self.checkpoint_ver.to_le_bytes());
        buf[8..16].copy_from_slice(&self.user_block_count.to_le_bytes());
        buf[16..24].copy_from_slice(&self.valid_block_count.to_le_bytes());
        buf[24..28].copy_from_slice(&self.rsvd_segment_count.to_le_bytes());
        buf[28..32].copy_from_slice(&self.overprov_segment_count.to_le_bytes());
        buf[32..36].copy_from_slice(&self.free_segment_count.to_le_bytes());

        for (i, &s) in self.cur_node_segno.iter().enumerate() {
            let off = 36 + i * 4;
            buf[off..off + 4].copy_from_slice(&s.to_le_bytes());
        }
        // cur_node_blkoff[8] @ 68 (8×u16=16B), cur_data_segno[8] @ 84,
        // cur_data_blkoff[8] @ 116 — 按 struct f2fs_checkpoint 紧凑布局。
        for (i, &b) in self.cur_node_blkoff.iter().enumerate() {
            let off = 68 + i * 2;
            buf[off..off + 2].copy_from_slice(&b.to_le_bytes());
        }
        for (i, &s) in self.cur_data_segno.iter().enumerate() {
            let off = 84 + i * 4;
            buf[off..off + 4].copy_from_slice(&s.to_le_bytes());
        }
        for (i, &b) in self.cur_data_blkoff.iter().enumerate() {
            let off = 116 + i * 2;
            buf[off..off + 2].copy_from_slice(&b.to_le_bytes());
        }
        // struct f2fs_checkpoint 紧凑布局 (数组均 8 元素):
        // cur_node_segno[8]@36(32B) cur_node_blkoff[8]@68(16B)
        // cur_data_segno[8]@84(32B) cur_data_blkoff[8]@116(16B) -> 132
        buf[132..136].copy_from_slice(&self.ckpt_flags.to_le_bytes());
        buf[136..140].copy_from_slice(&self.cp_pack_total_block_count.to_le_bytes());
        buf[140..144].copy_from_slice(&self.cp_pack_start_sum.to_le_bytes());
        buf[144..148].copy_from_slice(&self.valid_node_count.to_le_bytes());
        buf[148..152].copy_from_slice(&self.valid_inode_count.to_le_bytes());
        buf[152..156].copy_from_slice(&self.next_free_nid.to_le_bytes());
        buf[156..160].copy_from_slice(&self.sit_ver_bitmap_bytesize.to_le_bytes());
        buf[160..164].copy_from_slice(&self.nat_ver_bitmap_bytesize.to_le_bytes());
        buf[164..168].copy_from_slice(&(CP_CHKSUM_OFFSET as u32).to_le_bytes());
        buf[168..176].copy_from_slice(&self.elapsed_time.to_le_bytes());
        // alloc_type[MAX_ACTIVE_LOGS=16] @ 176 (紧随 elapsed_time 168..176)
        buf[176..192].copy_from_slice(&self.alloc_type);

        // sit_nat_version bitmap @ 192 (紧随 alloc_type)
        let bitmap_off = 192;
        let sit_end = bitmap_off + self.sit_bitmap.len();
        if sit_end <= CP_CHKSUM_OFFSET {
            buf[bitmap_off..sit_end].copy_from_slice(&self.sit_bitmap);
            let nat_end = sit_end + self.nat_bitmap.len();
            if nat_end <= CP_CHKSUM_OFFSET {
                buf[sit_end..nat_end].copy_from_slice(&self.nat_bitmap);
            }
        }

        let crc = crc32(&buf[..CP_CHKSUM_OFFSET]);
        buf[CP_CHKSUM_OFFSET..CP_CHKSUM_OFFSET + 4].copy_from_slice(&crc.to_le_bytes());
        buf
    }
}

/// compact summary 块内的 journal 区大小。
pub const SUM_JOURNAL_SIZE: usize = 507;
pub const NAT_JOURNAL_ENTRY_SIZE: usize = 13;

/// 构造 compact 格式的 DATA summary 块 (含 NAT/SIT journal + data summaries)。
pub fn build_compact_data_summary(
    nat_journal: &[(u32, u32)],        // (nid, block_addr) 列表
    sit_journal: &[(u32, u16)],        // (segno, vblocks) 列表
    sit_entries: &[([u8; 64], u64)],   // (valid_map, mtime)
    data_summaries: &[(u32, u8, u16)], // (nid, version, ofs_in_node)
) -> Vec<u8> {
    let mut buf = vec![0u8; F2FS_BLKSIZE];
    let mut off = 0usize;

    // 1. NAT journal (507 字节): n_nats(2) + 条目
    let n_nats = nat_journal.len().min(38) as u16;
    buf[off..off + 2].copy_from_slice(&n_nats.to_le_bytes());
    off += 2;
    for &(nid, block_addr) in nat_journal.iter().take(38) {
        buf[off] = 0; // version
        buf[off + 1..off + 5].copy_from_slice(&nid.to_le_bytes());
        buf[off + 5..off + 9].copy_from_slice(&block_addr.to_le_bytes());
        off += NAT_JOURNAL_ENTRY_SIZE;
    }
    off = SUM_JOURNAL_SIZE; // 跳到 SIT journal 区

    // 2. SIT journal (507 字节): n_sits(2) + 条目
    let n_sits = sit_journal.len().min(38) as u16;
    buf[off..off + 2].copy_from_slice(&n_sits.to_le_bytes());
    off += 2;
    for (i, &(segno, vblocks)) in sit_journal.iter().take(38).enumerate() {
        buf[off..off + 4].copy_from_slice(&segno.to_le_bytes());
        buf[off + 4..off + 6].copy_from_slice(&vblocks.to_le_bytes());
        if i < sit_entries.len() {
            buf[off + 6..off + 70].copy_from_slice(&sit_entries[i].0);
        }
        off += 74;
    }
    off = 2 * SUM_JOURNAL_SIZE; // 跳过两个 journal 区

    // 3. DATA summaries
    for &(nid, version, ofs) in data_summaries {
        if off + 7 > F2FS_BLKSIZE - 5 {
            break;
        }
        buf[off..off + 4].copy_from_slice(&nid.to_le_bytes());
        buf[off + 4] = version;
        buf[off + 5..off + 7].copy_from_slice(&ofs.to_le_bytes());
        off += 7;
    }
    let _ = CP_COMPACT_SUM_FLAG_W;
    buf
}
