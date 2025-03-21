# File Diff Extractor

一个用Rust编写的目录比较工具，可以生成可执行的补丁文件。

## Features

- 对比两个目录的文件差异
- 生成可执行的补丁文件（.exe）
- 补丁文件运行时会先验证目标目录是否正确
- 支持指定必要的验证文件，确保补丁应用在正确的目录中
- 高效的文件差异提取和应用

## Usage

### Create Patch

```bash
file-diff-extractor create --source <源文件目录> --target <目标文件目录> --output <补丁文件名> --check-files <验证文件1,验证文件2,...>
```

### Apply Patch

将生成的补丁文件放到需要更新的目录中，双击运行即可。补丁程序会先验证目录是否正确，然后应用文件更改。

## Build

```bash
cargo build --release
```

编译后的可执行文件将位于 `target/release/` 目录中。
