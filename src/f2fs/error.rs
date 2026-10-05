//! F2FS 解析错误类型。

use thiserror::Error;

#[derive(Error, Debug)]
pub enum F2fsError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid magic: expected {expected:#x}, got {got:#x}")]
    InvalidMagic { expected: u32, got: u32 },

    #[error("invalid block address: {0}")]
    InvalidBlock(u32),

    #[error("NAT entry not found: nid {0}")]
    NatNotFound(u32),

    #[error("decompression failed: {0}")]
    Decompression(String),

    #[error("invalid data: {0}")]
    InvalidData(String),
}

pub type Result<T> = std::result::Result<T, F2fsError>;
