//! F2FS 磁盘格式解析层。
//!
//! 模块划分:
//! - consts: 常量
//! - error: 错误类型
//! - types: 结构与解析
//! - volume: 卷读取后端 (超级块 / checkpoint / NAT)
//! - directory: 目录项
//! - file: 文件数据 (含压缩)
//! - xattr: 扩展属性

pub mod consts;
pub mod directory;
pub mod error;
pub mod file;
pub mod types;
pub mod volume;
pub mod write;
pub mod xattr;

pub use consts::*;
pub use error::{F2fsError, Result};
pub use types::{Block, BlockAddr, DirEntry, Inode, NatEntry, Nid, Superblock, XattrEntry};
pub use volume::F2fsVolume;
