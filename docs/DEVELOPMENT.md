# Folio Flow 开发说明

Folio Flow 是基于 [Dethon/folio](https://github.com/Dethon/folio) 的 Windows PDF 阅读与页面编辑分支，使用 Rust、Iced 和 PDFium。保留上游许可证与图标署名。

## 构建与验证

Rust 工具链由 `rust-toolchain.toml` 指定。Windows 构建脚本使用 PowerShell 7：

```powershell
pwsh -File scripts/fetch-pdfium.ps1
cargo fmt --check
cargo test -- --test-threads=1
cargo clippy --all-targets -- -D warnings
pwsh -File scripts/build-windows.ps1
```

最终程序始终为 `dist/Folio Flow/Folio Flow.exe`。Cargo 内部保留 `folio` 目标名称，统一构建脚本负责输出命名、许可证、PDFium 运行库、构建信息和独立运行检查。程序名称中的空格通过独立参数传递验证。运行中的旧实例不会被脚本自动结束。

Linux/WSL 跨编译入口保留为 `scripts/package-windows.sh`；安装器定义在 `installer/folio.nsi`，两者也使用 Folio Flow 的交付命名。

## GitHub 云端构建

推送到 `master` 会自动运行 [Windows release](https://github.com/kelsoz/Folio-Flow/actions/workflows/windows.yml)，也可以在该页面点击 **Run workflow** 手动运行。Windows x64 云端环境使用仓库指定的 Rust 版本，下载 PDFium，检查格式、运行测试和 Clippy，再执行同一本地打包脚本及导出检查。

成功后自动发布到 [GitHub Releases](https://github.com/kelsoz/Folio-Flow/releases/latest)，上传包含 Folio Flow.exe、PDFium 和许可证的 ZIP，以及 SHA-256 校验文件。Release 附件不受 Actions 的 30 天保留期限影响。标签采用 `v<软件版本>-build.<运行编号>.<重试编号>`，绑定本次构建的完整提交 SHA。先创建草稿并上传，成功后公开发布；失败不会留下公开的空版本。只允许 master 分支发布，不签名，不生成安装器。

## 需要保护的约定

- 单个 Engine worker 持有 PDFium，UI 不并发调用内核。
- 显示顺序、稳定 source PageId 编辑顺序、Viewer 物理顺序分开维护。
- 内部拖动先预览、松手提交；外部导入成功后一次性提交历史。
- 重排复用缩略图，旋转只刷新变化页，删除或插入保留幸存页缓存。
- 动画帧按需订阅；Tween 提交终值后才停止，避免透明面板与文字残留。
- 未保存内容的搜索、导出、打印和内存回收都使用当前编辑文档。
- 同路径保存采用临时文件发布，失败时恢复完整编辑快照。

## 本地材料

`target/` 为构建缓存、日志、截图和临时测试输出；`dist/` 为可运行产物；`vendor/pdfium/` 为单独获取的运行库。这些目录不提交。测试仅提交仓库内合成的 `tests/fixtures/sample.pdf`。

本机协作记录、私人测试样本与原始性能记录保留在本地，并通过 `.gitignore` 排除。README 中上游历史性能表不代表当前分支性能。

## 验收边界

功能回归不等同于真实桌面交互或全类型 PDF 验收。系统文件拖放、DPI、动画手感及 GUI 性能须实机确认；复杂 PDF 目录级结构、签名、表单等限制见使用说明。
