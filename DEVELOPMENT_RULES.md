# Ozon ERP 开发与交付准则

## 每次代码修改后的强制交付流程

任何会影响桌面程序行为、界面、数据库结构或采集逻辑的代码修改，均不得只停留在源码或 Debug 构建。完成修改后必须依次执行：

1. 运行与修改范围相符的测试，至少包括 `cargo check --locked`、Rust 相关测试和 `pnpm build`。
2. 必须使用 `desktop-next/scripts/build-tauri-release.cmd` 构建正式程序。禁止用单独的 `cargo build --release` 作为最终交付，因为它可能沿用开发配置并使 EXE 访问 `localhost:1420`。
3. 构建成功后关闭正在运行的旧版 `ozon-analytics-next.exe`。
4. 将 `desktop-next/src-tauri/target/release/ozon-analytics-next.exe` 覆盖复制到项目根目录 `ozon-analytics-next.exe`。
5. 核对根目录 EXE 的修改时间、文件大小和 SHA-256，确认其与 Release 产物一致。
6. 启动根目录 EXE，实际检查程序加载的是内嵌前端页面，而不是 `localhost:1420`。
7. 对本次修改对应的页面或功能执行一次可见验收；未完成真实验收时必须明确说明，不得把“源码已修改”等同于“正式程序已交付”。
8. 在 `PROJECT_ARCHITECTURE_AND_MIGRATION.md` 追加本次修改、测试、构建、覆盖和验收结果。

## 发布失败处理

- 如果窗口显示 `localhost 拒绝连接`，立即判定为错误发布构建，不要求用户启动开发服务器。
- 关闭错误进程，使用唯一正式构建入口重新构建、覆盖根目录 EXE 并重新验收。
- 不得继续让旧版 EXE、候选 EXE 或仅源码修改状态作为最终交付。

