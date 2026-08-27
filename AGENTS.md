# Repository instructions

本仓库的所有代码修改都必须遵循根目录 `DEVELOPMENT_RULES.md`。

尤其是桌面程序修改完成后，必须使用 `desktop-next/scripts/build-tauri-release.cmd` 生成嵌入正式前端的 Release EXE，覆盖根目录 `ozon-analytics-next.exe`，启动并验收。禁止将单独执行 `cargo build --release` 的产物直接作为最终交付。
