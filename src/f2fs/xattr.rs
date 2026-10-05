//! 扩展属性读取: inline xattr 与 xattr node。

use crate::f2fs::consts::*;
use crate::f2fs::error::Result;
use crate::f2fs::types::{Inode, Nid, XattrEntry};
use crate::f2fs::volume::F2fsVolume;
use std::io::{Read, Seek};

impl<R: Read + Seek + Send> F2fsVolume<R> {
    /// 读取 inode 的全部 xattr, 返回 (完整名, 值) 列表。
    pub fn read_xattrs(&self, inode: &Inode, nid: Nid) -> Result<Vec<(String, Vec<u8>)>> {
        let mut out = Vec::new();

        // 1. inline xattr (位于 inode 块尾部、footer 之前)
        if inode.inline & F2FS_INLINE_XATTR != 0 {
            let node = self.read_node(nid)?;
            let inline_size = DEFAULT_INLINE_XATTR_ADDRS * 4;
            let footer = NODE_FOOTER_SIZE;
            if node.len() > footer + inline_size {
                let off = node.len() - footer - inline_size;
                let data = &node[off..off + inline_size];
                // 前 4 字节为 xattr header (magic/refcount), 条目从第 5 字节起
                if data.len() > 4 {
                    parse_xattr_entries(&data[4..], &mut out)?;
                }
            }
        }

        // 2. xattr node (i_addr 中 xattr_nid 指向的独立块)
        if inode.xattr_nid != 0 {
            let node = self.read_node(Nid(inode.xattr_nid))?;
            // 头部 24 字节 + 条目区 + footer 24 字节
            if node.len() > NODE_FOOTER_SIZE * 2 {
                let end = node.len() - NODE_FOOTER_SIZE;
                parse_xattr_entries(&node[NODE_FOOTER_SIZE..end], &mut out)?;
            }
        }

        Ok(out)
    }
}

#[allow(clippy::unnecessary_wraps)]
fn parse_xattr_entries(data: &[u8], out: &mut Vec<(String, Vec<u8>)>) -> Result<()> {
    let mut off = 0;
    while off + 4 <= data.len() {
        // 全零表示到达末尾
        if data[off] == 0 && data[off + 1] == 0 {
            break;
        }
        match XattrEntry::from_bytes(&data[off..]) {
            Ok((entry, size)) => {
                out.push((entry.full_name(), entry.value));
                off += size;
            }
            Err(_) => break,
        }
    }
    Ok(())
}
