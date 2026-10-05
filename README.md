# f2fs-utils

F2FS tools for Android, written in Rust.

当前实现:

- **`extract.f2fs`** — 分解 F2FS 镜像,提取文件树并生成 Android 的
  `*_fs_config` / `*_file_contexts`。

## 用法

```bash
# 目录模式: 两份配置自动按 <partition>_fs_config / <partition>_file_contexts 命名
extract.f2fs -i <镜像> -o <分解输出目录> -c <配置输出目录>

# 文件模式: 分别指定两份配置的输出文件 (覆盖 -c 的自动命名)
extract.f2fs -i <镜像> -o <分解输出目录> --fs-config <文件> --file-contexts <文件>

# 混用: 只指定其中一个, 另一个回退到 -c 目录
extract.f2fs -i <镜像> -o <分解输出目录> -c <目录> --fs-config <文件>
```

示例:

```bash
extract.f2fs -i product.img -o ./product -c ./config
```

对包含 `/app`、`/etc` 的 `product.img`,结果:

```
./product/app/...
./product/etc/...
./config/product_fs_config
./config/product_file_contexts
```

`product_file_contexts`(每行: 路径 + SELinux context,均来自镜像内实际解析):

```
/product/app u:object_r:system_file:s0
/product/etc u:object_r:system_file:s0
```

`product_fs_config`(每行: `路径 uid gid 八进制mode`):

```
product/app 0 0 0755
product/etc 0 0 0755
```

## 构建

```bash
cargo build --release
```

产物在 `target/release/`。

> **产物命名约定**: Cargo 不允许二进制名包含 `.` 字符, 因此源码级产物名为
> `extract_f2fs`。Release/CI 构建会将其重命名为 Android 习惯的
> `extract.f2fs`。本地开发若需要该名称, 手动复制即可:
>
> ```bash
> cp target/release/extract_f2fs target/release/extract.f2fs
> ```

## 测试

```bash
cargo test
```

## Lint

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```
