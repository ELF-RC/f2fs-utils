//! inode / node 块构建器。

use crate::f2fs::consts::{
    DEF_ADDRS_PER_BLOCK, DEF_ADDRS_PER_INODE, F2FS_BLKSIZE, F2FS_DATA_EXIST, F2FS_EXTRA_ATTR,
    F2FS_INLINE_DATA, F2FS_INLINE_DENTRY, F2FS_INLINE_XATTR, F2FS_NAME_LEN,
    F2FS_XATTR_INDEX_SECURITY, NODE_FOOTER_SIZE, S_IFDIR, S_IFLNK, S_IFREG,
};
use crate::f2fs::write::consts::{
    COMPRESS_LZ4, DEFAULT_INLINE_XATTR_SIZE, EXTRA_ISIZE, NIDS_PER_BLOCK_W,
};
use crate::f2fs::write::crc::inode_checksum;
use crate::f2fs::write::types::{FileType, NodeFooter};

/// inline xattr 条目 (写入侧)。
#[derive(Debug, Clone)]
pub struct InlineXattrEntry {
    pub name_index: u8,
    pub name: Vec<u8>,
    pub value: Vec<u8>,
}

impl InlineXattrEntry {
    pub fn selinux(context: &str) -> Self {
        Self {
            name_index: F2FS_XATTR_INDEX_SECURITY,
            name: b"selinux".to_vec(),
            value: context.as_bytes().to_vec(),
        }
    }

    fn raw_size(&self) -> usize {
        4 + self.name.len() + self.value.len()
    }

    pub fn size(&self) -> usize {
        (self.raw_size() + 3) & !3
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.size());
        buf.push(self.name_index);
        buf.push(self.name.len() as u8);
        buf.extend_from_slice(&(self.value.len() as u16).to_le_bytes());
        buf.extend_from_slice(&self.name);
        buf.extend_from_slice(&self.value);
        while buf.len() % 4 != 0 {
            buf.push(0);
        }
        buf
    }
}

/// inode 构建器。
#[derive(Debug)]
pub struct InodeBuilder {
    mode: u16,
    uid: u32,
    gid: u32,
    links: u32,
    size: u64,
    blocks: u64,
    atime: u64,
    atime_nsec: u32,
    ctime: u64,
    ctime_nsec: u32,
    mtime: u64,
    mtime_nsec: u32,
    crtime: u64,
    crtime_nsec: u32,
    current_depth: u32,
    pino: u32,
    name: Vec<u8>,
    dir_level: u8,
    flags: u32,
    inline_flags: u8,
    xattr_nid: u32,
    addrs: Vec<u32>,
    nids: [u32; 5],
    has_extra_attr: bool,
    projid: u32,
    inline_xattrs: Vec<InlineXattrEntry>,
    /// inline 数据 (符号链接目标或小文件内容, 不占独立数据块)。
    inline_data: Option<Vec<u8>>,
    /// 压缩算法 (COMPRESS_LZ4 等)。
    compress_algorithm: u8,
    /// log2(压缩簇块数)。
    log_cluster_size: u8,
    /// 压缩标志位 (i_compress_flag, u16)。
    compress_flag: u16,
    /// 压缩后占的物理块总数 (i_compr_blocks)。
    compr_blocks: u32,
}

impl Default for InodeBuilder {
    fn default() -> Self {
        Self {
            mode: 0,
            uid: 0,
            gid: 0,
            links: 1,
            size: 0,
            blocks: 0,
            atime: 0,
            atime_nsec: 0,
            ctime: 0,
            ctime_nsec: 0,
            mtime: 0,
            mtime_nsec: 0,
            crtime: 0,
            crtime_nsec: 0,
            current_depth: 0,
            pino: 0,
            name: Vec::new(),
            dir_level: 0,
            flags: 0,
            inline_flags: 0,
            xattr_nid: 0,
            addrs: Vec::new(),
            nids: [0; 5],
            has_extra_attr: true,
            projid: 0,
            inline_xattrs: Vec::new(),
            inline_data: None,
            compress_algorithm: COMPRESS_LZ4,
            log_cluster_size: 2,
            compress_flag: 0,
            compr_blocks: 0,
        }
    }
}

impl InodeBuilder {
    pub fn new_dir(mode: u16, uid: u32, gid: u32) -> Self {
        let mut b = Self::default();
        b.mode = S_IFDIR | (mode & 0o7777);
        b.uid = uid;
        b.gid = gid;
        b.links = 2;
        b.has_extra_attr = false;
        b
    }

    pub fn new_file(mode: u16, uid: u32, gid: u32) -> Self {
        let mut b = Self::default();
        b.mode = S_IFREG | (mode & 0o7777);
        b.uid = uid;
        b.gid = gid;
        b.has_extra_attr = false;
        b
    }

    pub fn new_symlink(uid: u32, gid: u32) -> Self {
        let mut b = Self::default();
        b.mode = S_IFLNK | 0o777;
        b.uid = uid;
        b.gid = gid;
        b.inline_flags = F2FS_INLINE_DATA | F2FS_DATA_EXIST;
        b.has_extra_attr = false;
        b.blocks = 1;
        b
    }

    pub fn with_mode(mut self, mode: u16) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_links(mut self, links: u32) -> Self {
        self.links = links;
        self
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = size;
        self
    }

    pub fn with_blocks(mut self, blocks: u64) -> Self {
        self.blocks = blocks;
        self
    }

    pub fn with_timestamp(mut self, time: u64) -> Self {
        self.atime = time;
        self.ctime = time;
        self.mtime = time;
        self.crtime = time;
        self
    }

    /// 设置秒 + 纳秒, 四个时间字段统一 (保留源文件亚秒精度)。
    pub fn with_timestamp_nsecs(mut self, secs: u64, nsecs: u32) -> Self {
        self.atime = secs;
        self.atime_nsec = nsecs;
        self.ctime = secs;
        self.ctime_nsec = nsecs;
        self.mtime = secs;
        self.mtime_nsec = nsecs;
        self.crtime = secs;
        self.crtime_nsec = nsecs;
        self
    }

    pub fn with_pino(mut self, pino: u32) -> Self {
        self.pino = pino;
        self
    }

    pub fn with_name(mut self, name: &[u8]) -> Self {
        self.name = name.to_vec();
        self
    }

    pub fn with_depth(mut self, depth: u32) -> Self {
        self.current_depth = depth;
        self
    }

    pub fn with_addrs(mut self, addrs: Vec<u32>) -> Self {
        self.addrs = addrs;
        self
    }

    pub fn with_nids(mut self, nids: [u32; 5]) -> Self {
        self.nids = nids;
        self
    }

    pub fn with_symlink_target(mut self, target: &str) -> Self {
        let bytes = target.as_bytes().to_vec();
        self.size = bytes.len() as u64;
        self.inline_data = Some(bytes);
        self.inline_flags |= F2FS_INLINE_DATA | F2FS_DATA_EXIST;
        self
    }

    /// 内联小文件数据 (不占独立数据块); 自动置位 INLINE_DATA | DATA_EXIST。
    pub fn with_inline_data(mut self, data: Vec<u8>) -> Self {
        self.size = data.len() as u64;
        self.inline_data = Some(data);
        self.inline_flags |= F2FS_INLINE_DATA | F2FS_DATA_EXIST;
        self
    }

    pub fn with_compression(mut self, algo: u8, log_cluster_size: u8, compr_blocks: u32) -> Self {
        self.compress_algorithm = algo;
        self.log_cluster_size = log_cluster_size;
        self.compr_blocks = compr_blocks;
        self.compress_flag = 1; // F2FS Compress_File_Flag
        self.has_extra_attr = true;
        self
    }

    pub fn with_selinux_context(mut self, context: &str) -> Self {
        self.has_extra_attr = true;
        self.inline_flags |= F2FS_EXTRA_ATTR;
        self.inline_xattrs.push(InlineXattrEntry::selinux(context));
        self.inline_flags |= F2FS_INLINE_XATTR;
        self
    }

    /// 实际可用的直接块地址数 (随 extra_attr / inline_xattr 收缩)。
    fn addrs_per_inode(&self) -> usize {
        if self.has_extra_attr {
            DEF_ADDRS_PER_INODE - (EXTRA_ISIZE as usize / 4) - DEFAULT_INLINE_XATTR_SIZE as usize
        } else {
            DEF_ADDRS_PER_INODE
        }
    }

    /// 构建 inode node 块 (4KiB)。
    pub fn build(&self, nid: u32, ino: u32, cp_ver: u64) -> [u8; F2FS_BLKSIZE] {
        let mut buf = [0u8; F2FS_BLKSIZE];

        buf[..2].copy_from_slice(&self.mode.to_le_bytes());
        buf[3] = self.inline_flags;
        buf[4..8].copy_from_slice(&self.uid.to_le_bytes());
        buf[8..12].copy_from_slice(&self.gid.to_le_bytes());
        buf[12..16].copy_from_slice(&self.links.to_le_bytes());
        buf[16..24].copy_from_slice(&self.size.to_le_bytes());
        buf[24..32].copy_from_slice(&self.blocks.to_le_bytes());
        buf[32..40].copy_from_slice(&self.atime.to_le_bytes());
        buf[40..48].copy_from_slice(&self.ctime.to_le_bytes());
        buf[48..56].copy_from_slice(&self.mtime.to_le_bytes());
        buf[56..60].copy_from_slice(&self.atime_nsec.to_le_bytes());
        buf[60..64].copy_from_slice(&self.ctime_nsec.to_le_bytes());
        buf[64..68].copy_from_slice(&self.mtime_nsec.to_le_bytes());
        buf[72..76].copy_from_slice(&self.current_depth.to_le_bytes());
        buf[76..80].copy_from_slice(&self.xattr_nid.to_le_bytes());
        buf[80..84].copy_from_slice(&self.flags.to_le_bytes());
        buf[84..88].copy_from_slice(&self.pino.to_le_bytes());

        let namelen = self.name.len().min(F2FS_NAME_LEN) as u32;
        buf[88..92].copy_from_slice(&namelen.to_le_bytes());
        buf[92..92 + namelen as usize].copy_from_slice(&self.name[..namelen as usize]);

        buf[347] = self.dir_level;

        // extra_attr 区域 (偏移 360)
        if self.has_extra_attr {
            buf[360..362].copy_from_slice(&EXTRA_ISIZE.to_le_bytes());
            buf[362..364].copy_from_slice(&DEFAULT_INLINE_XATTR_SIZE.to_le_bytes());
            buf[364..368].copy_from_slice(&self.projid.to_le_bytes());
            // i_inode_checksum @ 368..372, 稍后填
            buf[372..380].copy_from_slice(&self.crtime.to_le_bytes());
            buf[380..384].copy_from_slice(&self.crtime_nsec.to_le_bytes());
            // 压缩字段 (384..396)
            buf[384..392].copy_from_slice(&u64::from(self.compr_blocks).to_le_bytes());
            buf[392] = self.compress_algorithm;
            buf[393] = self.log_cluster_size;
            buf[394..396].copy_from_slice(&self.compress_flag.to_le_bytes());
        }

        let addr_offset = if self.has_extra_attr { 396 } else { 360 };

        if let Some(ref data) = self.inline_data {
            // inline data: 保留槽 + 实际数据
            let reserved = addr_offset;
            buf[reserved..reserved + 4].copy_from_slice(&0u32.to_le_bytes());
            let data_off = reserved + 4;
            let max = F2FS_BLKSIZE - data_off - NODE_FOOTER_SIZE;
            let len = data.len().min(max);
            buf[data_off..data_off + len].copy_from_slice(&data[..len]);
        } else {
            let max = self.addrs_per_inode();
            for (i, &addr) in self.addrs.iter().take(max).enumerate() {
                let off = addr_offset + i * 4;
                buf[off..off + 4].copy_from_slice(&addr.to_le_bytes());
            }
        }

        // nids[5] 位于 360 + DEF_ADDRS_PER_INODE * 4
        let nid_offset = 360 + DEF_ADDRS_PER_INODE * 4;
        for (i, &n) in self.nids.iter().enumerate() {
            let off = nid_offset + i * 4;
            buf[off..off + 4].copy_from_slice(&n.to_le_bytes());
        }

        // inline xattr (footer 之前)
        if !self.inline_xattrs.is_empty() && self.has_extra_attr {
            let xattr_bytes = DEFAULT_INLINE_XATTR_SIZE as usize * 4;
            let xattr_start = F2FS_BLKSIZE - NODE_FOOTER_SIZE - xattr_bytes;
            let mut data = Vec::new();
            data.extend_from_slice(&0xF2F5_2011u32.to_le_bytes()); // xattr magic
            for entry in &self.inline_xattrs {
                data.extend_from_slice(&entry.to_bytes());
            }
            data.extend_from_slice(&[0u8; 4]); // 终止标记
            let len = data.len().min(xattr_bytes);
            buf[xattr_start..xattr_start + len].copy_from_slice(&data[..len]);
        }

        // node footer
        let footer = NodeFooter {
            nid,
            ino,
            flag: 0,
            cp_ver,
            next_blkaddr: 0,
        };
        buf[F2FS_BLKSIZE - NODE_FOOTER_SIZE..].copy_from_slice(&footer.to_bytes());

        // inode 校验和
        if self.has_extra_attr {
            let crc = inode_checksum(ino, &buf);
            buf[368..372].copy_from_slice(&crc.to_le_bytes());
        }

        let _ = F2FS_INLINE_DENTRY;
        buf
    }
}

/// direct node 块构建器 (纯块地址数组 + footer)。
#[derive(Debug, Default)]
pub struct DirectNodeBuilder {
    addrs: Vec<u32>,
}

impl DirectNodeBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_addrs(mut self, addrs: Vec<u32>) -> Self {
        self.addrs = addrs;
        self
    }

    pub fn build(&self, nid: u32, ino: u32, cp_ver: u64) -> [u8; F2FS_BLKSIZE] {
        let mut buf = [0u8; F2FS_BLKSIZE];
        for (i, &addr) in self.addrs.iter().take(DEF_ADDRS_PER_BLOCK).enumerate() {
            let off = i * 4;
            buf[off..off + 4].copy_from_slice(&addr.to_le_bytes());
        }
        let footer = NodeFooter {
            nid,
            ino,
            flag: 0,
            cp_ver,
            next_blkaddr: 0,
        };
        buf[F2FS_BLKSIZE - NODE_FOOTER_SIZE..].copy_from_slice(&footer.to_bytes());
        let _ = NIDS_PER_BLOCK_W;
        buf
    }
}

/// indirect node 块构建器 (纯 nid 数组 + footer)。
#[derive(Debug, Default)]
pub struct IndirectNodeBuilder {
    nids: Vec<u32>,
}

impl IndirectNodeBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_nid(&mut self, nid: u32) {
        if self.nids.len() < NIDS_PER_BLOCK_W {
            self.nids.push(nid);
        }
    }

    pub fn len(&self) -> usize {
        self.nids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nids.is_empty()
    }

    pub fn build(&self, nid: u32, ino: u32, cp_ver: u64) -> [u8; F2FS_BLKSIZE] {
        let mut buf = [0u8; F2FS_BLKSIZE];
        for (i, &n) in self.nids.iter().take(NIDS_PER_BLOCK_W).enumerate() {
            let off = i * 4;
            buf[off..off + 4].copy_from_slice(&n.to_le_bytes());
        }
        let footer = NodeFooter {
            nid,
            ino,
            flag: 0,
            cp_ver,
            next_blkaddr: 0,
        };
        buf[F2FS_BLKSIZE - NODE_FOOTER_SIZE..].copy_from_slice(&footer.to_bytes());
        buf
    }
}

/// 从 mode 推断文件类型 (供目录项使用)。
pub fn file_type_from_mode(mode: u16) -> u8 {
    FileType::from_mode(mode).as_u8()
}
