//! extract.f2fs — F2FS 镜像分解工具。
//!
//! 用法:
//! ```text
//! extract.f2fs -i <镜像> -o <分解输出目录> -c <配置输出目录>
//! extract.f2fs -i <镜像> -o <分解输出目录> --fs-config <文件> --file-contexts <文件>
//! ```
//!
//! 示例:
//! ```text
//! extract.f2fs -i product.img -o ./product -c ./config
//! ```

use clap::Parser;
use f2fs_utils::extract::{ExtractConfig, extract};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "extract.f2fs",
    about = "Extract an F2FS image and emit Android fs_config / file_contexts",
    version
)]
struct Cli {
    /// 输入镜像路径
    #[arg(short, long)]
    input: PathBuf,

    /// 分解输出目录 (文件树落盘根)
    #[arg(short, long)]
    output: PathBuf,

    /// 配置输出目录 (未显式指定文件时, 按 `<partition>_fs_config` /
    /// `<partition>_file_contexts` 自动命名)
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// 显式指定 `fs_config` 文件路径 (覆盖 `-c` 的自动命名)
    #[arg(long)]
    fs_config: Option<PathBuf>,

    /// 显式指定 `file_contexts` 文件路径 (覆盖 `-c` 的自动命名)
    #[arg(long = "file-contexts")]
    file_contexts: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("extract.f2fs: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let cfg = ExtractConfig {
        input_image: cli
            .input
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("input path is not valid UTF-8"))?
            .to_string(),
        output_dir: cli.output,
        config_dir: cli.config,
        fs_config_path: cli.fs_config,
        file_contexts_path: cli.file_contexts,
    };
    extract(&cfg)
}
