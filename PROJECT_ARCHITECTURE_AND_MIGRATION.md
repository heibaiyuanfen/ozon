# Ozon / WB ERP 项目结构、业务逻辑与迁移记录

> 本文档是本项目唯一的结构说明与迁移台账。自 2026-08-25 起，每次功能、数据口径、数据库、接口、缓存、构建方式或迁移进度发生变化，都必须在文末“更新记录”中追加一条记录，并同步修改对应章节。禁止只改代码不更新本文档。

## 1. 项目目标

本项目把最初的 Python 桌面版 Ozon Seller Analytics、RFBS 上品工具及 WB 工作台，逐步重构为一个可直接双击运行的 React + Tauri + Rust 本地 ERP。

核心目标：

- 保留 Python 旧版已经验证过的数据口径、费用归类、利润公式和操作流程。
- Ozon 本土店、跨境店和 WB 数据严格隔离，避免店铺、平台和汇率混用。
- API 数据写入本地 SQLite，关闭软件后仍然保留，并支持分批、断点、去重同步。
- 报表优先读取缓存，耗时同步和计算在后台执行，避免阻塞界面。
- 敏感 API 密钥通过 Windows DPAPI 加密保存，不写入源码或导出到版本库。
- 最终交付物是 `ozon-analytics-next.exe`，不依赖额外的 CMD 启动器。

## 2. 顶层目录

```text
Ozon_analytics_ui_upgrade_source/
├─ desktop-next/                 # 当前主版本：React + Tauri + Rust
│  ├─ src/                       # React/TypeScript 前端
│  ├─ src-tauri/src/             # Rust 后端、SQLite、API 与业务计算
│  ├─ scripts/                   # 前端和正式版构建脚本
│  ├─ dist/                      # 前端构建产物，不是源码
│  └─ node_modules/              # Node 依赖，不是源码
├─ ozon_app/                     # 最初的 Python Ozon Analytics 旧版
├─ legacy-sources/
│  └─ rfbs-listing-tool/         # RFBS/WB 上品工具的只读迁移基线
├─ tests/                        # Python 旧版测试
├─ tools/                        # 辅助脚本
├─ data/                         # 本地运行数据；不应作为业务源码修改
├─ exports/                      # 导出文件
├─ main.py                       # Python 旧版入口
├─ build_exe.ps1                 # Python 旧版打包脚本
└─ PROJECT_ARCHITECTURE_AND_MIGRATION.md
```

### 2.1 原始源码快照规则

`legacy-sources/rfbs-listing-tool/` 保存从以下目录复制的迁移基线：

```text
D:\OZON 跟卖软件\新建文件夹\新建文件夹 (2)\RFBS上品工具
```

已复制：Python 模块、测试、`config.example.json`、旧版 README 和 PyInstaller spec。

明确未复制：

- `config.json` 和 `ozon-api-configs-*.json`：可能包含真实密钥。
- `browser_profile*`：包含浏览器会话、Cookie 和大量缓存。
- `workspace_state.json`、`auto_jobs.json`：属于用户运行状态和任务数据。
- `keywords.db`、`产品台账.xlsx`：属于业务数据，不是源码。
- `.runtime-deps`、`__pycache__`、`cache`、`输出`：依赖、缓存或构建产物。

原始快照用于逐函数对照，原则上不直接改写；新逻辑应迁移到 `desktop-next`。

## 3. 当前主程序架构

```text
React 页面
   │ 通过 bridge.ts 调用 Tauri command
   ▼
Rust 命令层（lib.rs / listing.rs / wb.rs / insights.rs）
   ├─ Ozon Seller / Performance / Finance API
   ├─ WB API
   ├─ 飞书与本地导入导出
   ├─ 业务聚合、利润与费用口径
   └─ 后台线程、同步进度与错误日志
   ▼
SQLite 本地数据库（按店铺隔离）
   ├─ 原始同步数据
   ├─ 产品成本与设置
   ├─ 报表缓存
   ├─ 同步断点
   └─ 上品任务草稿
```

### 3.1 前端

- `desktop-next/src/App.tsx`：应用路由、Ozon/WB 工作区和主框架入口。
- `desktop-next/src/OperationsPages.tsx`：报表、同步、库存、上品、WB 等主要业务页面。
- `desktop-next/src/Phase2Pages.tsx`：订单、商品、广告等第二阶段页面。
- `desktop-next/src/ProductInsights.tsx`：单品趋势和集群分析。
- `desktop-next/src/bridge.ts`：前端到 Rust Tauri command 的唯一调用边界。
- `desktop-next/src/types.ts`：前后端共享的数据结构声明。
- `*.css`：页面样式；长列表应保持分页或虚拟化，避免一次渲染全部数据。

前端不直接访问 Ozon/WB API，也不负责持久化。页面只负责输入、状态展示和调用后端。

### 3.2 Rust 后端

- `desktop-next/src-tauri/src/lib.rs`：主数据库、Ozon API、同步中心、报表、库存、订单及大部分 Tauri 命令。
- `listing.rs`：RFBS 上品台账、ROI 核价、参考商品采集、上品草稿和迁移任务。
- `wb.rs`：WB 独立数据库、订单、广告、产品成本、仓库与利润逻辑。
- `secrets.rs`：Windows DPAPI 加密/解密。
- `insights.rs`：产品洞察相关聚合。
- `main.rs`：Tauri 程序入口。

耗时网络请求、Excel 读取、库存同步和大数据计算应通过 `spawn_blocking` 或异步任务执行，不允许在 UI 主线程同步完成。

### 3.3 本地数据库和缓存

主要数据类型包括：

- Seller 销量、订单和商品日数据。
- Performance 广告活动、SKU 明细和日趋势。
- Finance 逐笔应计与费用字段。
- 库存总量、仓库库存和在途/预留字段。
- 产品成本、尺寸、重量、运输配置和汇率设置。
- 同步日志、`sync_progress` 断点和数据覆盖范围。
- `business_report_cache`、`analytics_detail_cache` 报表缓存。
- `listing_jobs` 上品草稿、阶段、状态、错误和 JSON payload。
- WB 独立订单、广告、成本和仓库表。

缓存原则：

1. API 每成功取得一批数据就立即事务写入，不能等全部完成才保存。
2. 使用业务唯一键 UPSERT，已缓存数据不重复插入。
3. 同步失败不删除历史缓存；再次同步从断点继续。
4. 报表缓存用数据指纹判断是否失效，而不是每次打开软件全部重算。
5. 默认至少保留最近三个月销售数据；清理更早数据必须由用户明确操作。

## 4. 业务模块与口径

### 4.1 数据同步中心

负责 Seller、Performance、Finance、库存和 WB 数据同步。用户可选择日期范围。同步日志记录来源、时间、状态、行数和错误；HTTP 429 应退避重试，DPAPI 失败应提示重新保存密钥。

### 4.2 经营总览与产品报告

销售额、销量和订单来自 Seller 本地缓存；广告来自 Performance；成本来自店铺产品成本；Finance 仅用于已结算对账。不同来源日期和口径不可直接混为一个字段。单品页面默认只展示最近两个月有销量的 SKU。

### 4.3 月度盈亏

支持月份选择和历史月份计算。核心恒等式：

```text
Finance 已结算净额
= 销售/退货应计
+ 应计费用
+ 其他收入/调整
```

平台佣金、配送、退货物流、仓储包装、收单、税额和提现手续费必须按 Finance API 原字段归类并提供明细。税率和回款手续费是店铺可编辑设置，不应写死。经营预估与 Finance 已结算属于不同口径，界面必须明确标注。

### 4.4 跨境店利润

跨境销售首先按订单/Seller 的 RUB 金额汇总，再使用“跨境店 RUB/CNY 汇率”换算。采购成本和预估运费使用 CNY。跨境店与本土店分别保存汇率，商品中心、订单、报表和利润计算使用当前店铺所属汇率。佣金应优先使用该 SKU 的 Finance 历史费率；没有历史样本时必须显示“费率不足”，不能静默当作零。

### 4.5 库存

后台 Ozon 页面“仓库库存”与 API 的可售、预留、在途、已申请不是同一字段。软件应分别展示后台总库存、可售、预留、在途、已申请和仓库库存，并注明来源。同步在后台执行，页面切换只读取本地快照。

### 4.6 广告运营

支持日期范围、活动名称筛选、活动表、趋势、曝光—点击—订单漏斗和 SKU 明细。活动汇总不能与 SKU 明细重复计费。当前 Ozon 产品级广告 API 只能稳定补充最近日期时，历史缺口必须明确展示，不能假装为零。

### 4.7 WB 工作区

WB 使用独立 Token、数据库和导航，不混入 Ozon 表。目标是拥有与 Ozon 对应的经营总览、订单、商品成本、广告、库存、利润、数据同步、API 导入导出和飞书协作模块。目前仅部分模块完成，详见迁移进度。

### 4.8 RFBS 上品

旧版流程：参考商品识别/采集 → 类目推荐 → 属性定义与字典值 → 图片与俄文文本 → ROI 核价 → 必填校验 → 提交 Ozon → 查询导入任务 → 失败重试/断点续跑 → 产品台账回写。

新版本已建立持久化 `listing_jobs` 工作流；阶段定义：

- `0`：待采集。
- `1`：参考商品信息已取得或手工填写。
- `2`：类目和 type 已填写。
- `3`：属性结构已填写，状态为 `ready`。

## 5. Python 旧版到新版本的迁移映射

| 旧版来源 | 主要作用 | 新版位置 | 状态 |
|---|---|---|---|
| 根目录 `ozon_app/` | Ozon 分析、利润、订单、同步 | `desktop-next/src` + `src-tauri/src/lib.rs` | 主体已迁移，持续校对口径 |
| `core.py` | 参考商品、类目、属性、提交结构、ROI | `src-tauri/src/listing.rs` | 部分迁移 |
| `product_ledger.py` | 产品台账、成本与任务字段 | `listing.rs` + 产品成本表 | 成本/台账字段已迁移，回写待完善 |
| `services.py` | Ozon API 和后台任务 | `lib.rs` / `listing.rs` | 同步主体已迁移，上品 API 待继续 |
| `app.py` | RFBS 桌面 UI 与自动任务 | `OperationsPages.tsx` | 草稿与核价已迁移，完整向导待迁移 |
| `wb.py` | WB 数据与利润 | `src-tauri/src/wb.rs` | 部分迁移 |
| `wb_pricing.py` | WB 核价 | `wb.rs` | 核心规则部分迁移 |
| `wb_source.py` | WB 商品来源/采集 | WB 工作区 | 未完整迁移 |
| `wb_ui.py` | WB 独立 UI | React WB 路由 | 框架已迁移，模块待补齐 |

## 6. 当前迁移进度（2026-08-25）

### 已完成或已接入

- React + Tauri 主程序，可直接双击 Release EXE。
- Ozon/WB 工作区切换和店铺隔离基础。
- Seller、Performance、Finance、库存同步及本地持久化基础。
- 日期范围同步、同步日志、部分断点与 UPSERT 去重。
- 经营总览、订单、商品、广告、产品报告、月度盈亏、周报和跨境利润页面框架。
- 月度盈亏 Finance 费用明细、缺成本 SKU 明细和月份选择。
- 税率、回款手续费及本土/跨境汇率可配置基础。
- 库存同步转后台，库存快照和索引优化。
- 报表缓存和数据指纹缓存。
- API、产品成本及 WB API 导入导出基础。
- RFBS 原版分段运费、2% 物流佣金、广告和货损 ROI 核价。
- RFBS 产品台账成本、尺寸、重量读取基础。
- Ozon 链接/Артикул 标准化。
- 上品草稿持久化、编辑、阶段校验、错误保留和重试。
- 参考商品后台直连采集及 JSON-LD 标题、描述、图片、属性解析。

### 已完成校对（2026-08-25）

- 每日产品、产品利润、周报和跨境利润已与 Python 旧版核心字段及公式对照；经营预估与 Finance 已结算继续分列。
- Finance 仅将唯一 SKU 记录精确归属；多 SKU 和店铺级费用保留未分摊，本轮修复 Rust 多 SKU 顶层字段回退问题。
- Ozon 商品图片已通过商品目录/详情缓存，订单中心按 SKU 懒加载展示。
- 库存已分列展示 Analytics 可售/在途/已申请与 Seller 后台总库存/预留，不混用来源。
- 广告活动总额与 SKU 明细防重复计费；SKU 接口允许日期内增量补齐，历史缺口不伪造。
- WB 已拆分每日经营、订单、广告、仓库库存、利润、成本、同步和设置模块，并迁移至 2026 新 Analytics 库存接口。
- 竞品直连失败时支持系统浏览器验证和用户手动导入验证后 HTML，不读取 Cookie。
- ECharts 改为按需加载并完成 Rollup 分块，入口包降至约 124 KB，所有块均低于 500 KB。

详细审计、口径与风险见 `MIGRATION_COMPLETION_AUDIT_2026-08-25.md`。

### 仍受外部 API 限制

- WB 2026 新 Analytics 库存报表要求 Token 具备 Analytics 权限；无权限时保留上次缓存并明确提示，不回退到已停用接口。
- Ozon Performance 历史 SKU 明细受服务端可查询日期限制，客户端只保留和增量补齐可获得的数据。

### 尚未完成的 RFBS 上品关键流程

- 属性定义、组合属性和字典值加载。
- 参考属性到 Ozon 属性的保守匹配。
- 必填属性、中文自由文本、标签和图片提交前校验。
- `/v3/product/import` 正式提交。
- 导入任务状态轮询、错误明细和断点续跑。
- 成功结果回写产品台账。

## 7. 构建、测试与交付

正式版构建入口：

```text
desktop-next\scripts\build-tauri-release.cmd
```

正式 EXE：

```text
desktop-next\src-tauri\target\release\ozon-analytics-next.exe
```

验证要求：

1. `cargo test` 必须通过。
2. Vite/TypeScript 正式构建必须通过。
3. Release Rust 编译必须通过。
4. 实际启动 EXE，进程不能立即退出且窗口应响应。
5. 数据口径变化必须增加单元测试或对照样本。

## 8. 后续开发纪律

- 先查找 Python 对应函数、字段、公式和测试，再修改新版本。
- 不凭页面截图猜测财务公式；金额差异必须追溯到源记录和归类字段。
- 店铺 ID、平台、日期范围和币种必须作为查询条件的一部分。
- 新增同步必须分批落盘、支持幂等和失败续跑。
- 不把 API 密钥、浏览器 Cookie、真实台账或数据库提交到源码目录。
- 不删除旧版逻辑基线；确认新旧测试一致后再标记为“完成迁移”。
- 每次更新本文档：修改相关结构/进度章节，并在下方追加日期、范围、文件、数据口径、测试结果和剩余风险。

## 9. 更新记录

### 2026-08-25：建立公开 GitHub 源码仓库

- 目标远程仓库：`https://github.com/heibaiyuanfen/ozon.git`，主分支为 `main`。
- 公开仓库只上传源码、测试、示例配置和项目文档。
- `node_modules` 与 `data` 继续保留在本机，但通过 `.gitignore` 排除，不上传依赖副本、店铺数据库、API 密钥、浏览器会话、Rust 编译缓存、EXE/PDB 和真实导出数据。
- 上传前完成大文件和凭据模式扫描；测试中的 `sk-very-secret-value` 是用于验证错误信息脱敏的固定假值，不是真实密钥。

### 2026-08-25：清理重复构建与编译缓存

- 删除 Rust `target/debug` 调试缓存、重复的 `target-final-release` 和 Release 的 `deps`、`build`、PDB、指纹及其他可重建中间文件。
- 删除重复旧版 `build-wb-release` EXE、Vite `dist` 和 TypeScript 增量缓存。
- 明确保留 `desktop-next/node_modules`、整个 `data` 业务数据目录、全部源码和当前正式 `target/release/ozon-analytics-next.exe`。
- 项目体积由约 9.0 GB 降至约 0.33 GB；后续重新构建时 Rust/Vite 会按需重新生成中间文件。

### 2026-08-25：建立项目总文档与原始源码基线

- 新增 `legacy-sources/rfbs-listing-tool/`，复制 RFBS/WB 上品 Python 源码、测试、示例配置、旧 README 和打包 spec。
- 排除真实 API 配置、浏览器 profile、任务状态、数据库、Excel、缓存、依赖和输出目录。
- `.gitignore` 新增原始迁移目录的凭据和运行数据保护规则，防止后续误复制、误提交。
- 新增本文档，记录整体架构、业务口径、目录职责、迁移映射、当前进度和后续纪律。

### 2026-08-25：RFBS 上品第二阶段迁移

- `listing.rs` 新增上品任务持久化、Ozon 链接/Артикул 标准化、草稿编辑和阶段校验。
- React 上品页面新增任务列表、采集、编辑、错误重试及普通/组合属性 JSON 编辑。
- 新增后台参考页采集，解析 JSON-LD 标题、描述、图片和页面属性；失败不覆盖草稿。
- 保留原版 ROI 分段运费、佣金、广告、货损核价逻辑。

### 2026-08-26：RFBS 实时类目目录迁移

- 对照旧版 `flatten_categories` 与 `rank_categories`，迁移禁用节点过滤、继承类目 ID、旧/新 type 结构、俄文词干及 0.72 近似词阈值。
- 新增当前店铺隔离的 `listing_catalog_cache`，通过 `/v1/description-category/tree` 下载 `ZH_HANS` 实时目录并按 `(description_category_id, type_id)` 去重。
- 上品草稿编辑器新增实时类目刷新、中俄文搜索、候选路径展示以及类目/type 成对选择，停止依赖人工猜测 ID。
- 类目下载和搜索在后台任务执行；缓存不包含 API 密钥。
- 新增类目树兼容与俄文词干测试。上品相关测试全部通过；全套测试仅 DPAPI 沙箱上下文测试失败。
- 剩余关键路径从属性定义/字典值加载开始，正式商品导入仍保持关闭，避免前置校验未完成时产生不可逆远程写入。

### 2026-08-26：RFBS Ozon 验证页采集恢复

- 核实旧版 `scrape_reference_browser`：Playwright 使用独立持久化 profile，按 Edge/Chrome 回退，等待人工验证和图库稳定，校验最终 Артикул 后提取标题、描述、高清图、页面参数与标签。
- 新版直连发生重定向、验证或读取失败时自动打开商品页，并在任务错误中给出恢复指引；失败不覆盖原 payload。
- 草稿编辑器新增“打开 Edge/浏览器”和“导入验证页并继续”。用户保存验证后 HTML，应用核对 canonical Артикул、解析商品结构并要求至少一张图片后才推进阶段。
- 应用不读取浏览器 Cookie、密码或日常 profile；只读取用户明确选择的 HTML 文件。

### 2026-08-26：RFBS 专用浏览器自动采集

- 引入 Rust CDP 浏览器控制组件并直接编译进桌面程序，不依赖用户安装 Python、Node 或 Playwright。
- 点击“采集”后先尝试 HTTP 直连；重定向、验证页或缺少图库时，自动按 Edge/Chrome 安装路径启动可见的专用浏览器和独立 `listing_browser_profile`。
- 用户仅在 Ozon 显示验证时完成人工验证；程序每秒轮询 DOM，自动识别验证页、读取 JSON-LD、标题、描述、商品参数和最多 20 张高清图库图片。
- 图库连续稳定后自动校验最终 URL 的 Артикул、写回任务并关闭专用浏览器，不要求人工保存网页。
- 手工 HTML 导入继续保留为浏览器运行时异常时的恢复入口，不属于正常自动上品主流程。
- 专用 profile 与业务数据库一样保留在本地数据目录，不提交源码；程序不连接或读取用户日常 Edge/Chrome profile。
- Rust 单元测试 9 项通过，前端和 Release 构建通过，EXE 实际启动验证通过。

### 2026-08-25：经营口径、WB、竞品与前端性能补齐

- 逐项复核报表、Finance、图片、库存和广告的新旧版调用链与字段来源。
- 修复 Finance 多 SKU 记录错误回退到顶层 SKU 的归属问题。
- WB 新增订单、商品级广告和仓库库存独立页签，接入 2026 新 Analytics 库存端点并保留权限失败时的旧缓存。
- 竞品新增系统浏览器验证与本地 HTML 安全导入恢复流程。
- ECharts 使用 Core 按需注册，Rollup 拆分 React、图表、图标和通用依赖，消除 500 KB 构建警告。
- 新增 `MIGRATION_COMPLETION_AUDIT_2026-08-25.md` 保存完整审计、验证和外部 API 风险。

### 2026-08-26：WB 首页对齐 Ozon ERP 看板

- 将 WB 首页从四卡片和销售柱状图改为 Ozon ERP 同款双摘要卡、指标条、折线趋势、经营健康度和同步状态布局。
- 销售额使用蓝色面积折线，广告费使用右轴橙色折线，暂估利润使用绿色虚线，避免图例压住坐标轴。
- 本地诊断确认订单和库存已有缓存、广告表为 0 行；新增 Token“推广”权限与广告活动状态提示。
- 同步返回增加活动数、商品广告行数和零活动诊断；活动 ID 解析兼容 WB 不同响应字段。

### 2026-08-26：广告活动双击监控、账户控制、效果日志与 AI 分析

- 目的：广告模块固定纳入最近两个月内实际产生消耗的活动；双击活动进入独立监控窗口，查看每日、每周、每月消耗、归因销售额、订单和 ROAS。
- 修改模块/文件：`desktop-next/src/App.tsx`、`types.ts`、`bridge.ts`、`performance.css` 及 `src-tauri/src/lib.rs`。
- 数据口径或数据库变化：广告页顶部汇总仍严格跟随用户所选日期；活动清单单独按最近两个月 `SUM(spend) > 0` 筛选。新增 `campaign_action_logs` 表，保存活动开启、关闭、周预算调整的操作前状态、操作后状态、操作前 7 天效果，以及操作日至今效果。活动总计优先读取 `sku=''` 聚合记录，缺少时才汇总 SKU 明细，避免重复计数。
- API 控制：依据 Ozon Performance API 当前接口接入活动 `activate`、`deactivate` 和 `PATCH weeklyBudget`；弃用的 `dailyBudget` 未继续使用。每次写操作必须同时通过浏览器确认和输入“确认执行”，AI 建议不会自动执行。
- AI 分析：读取该活动近两个月日级效果与 ERP 独立操作日志，分析预算/开关变更前后的消耗、销售额和 ROAS，并输出事实、推断、风险与建议。
- 缓存与性能影响：监控和日志首先读取本地 SQLite；仅用户确认账户控制时访问 Ozon，操作结束后回读活动状态和周预算。活动清单查询使用现有 `ad_daily(day,campaign_id,sku)` 索引范围。
- 测试和构建结果：TypeScript 类型检查、Vite 正式构建和 Rust `cargo check` 已通过；Rust 11 项单元测试全部通过；Tauri Release EXE 已成功生成，并实际启动持续运行 8 秒。EXE 大小 25,113,600 字节，SHA-256 为 `8228A90130B7E9B8ED410A8E69AB6100B5871FF4A079C50EE6A65435AA704CEF`。安装器阶段因当前环境禁止下载 WiX（Windows socket error 10013）而跳过，不影响直接双击 EXE。
- 尚存风险/下一步：Ozon 账户需具备 Performance 广告写权限；API 拒绝时错误会写入该活动日志，保留原缓存状态。效果归因只能基于操作后已同步到本地的数据，不能证明单一因果关系。

#### 同日补充：经营总览同款广告效果看板与实时周预算

- 将监控窗口的逐行花费进度条替换为经营总览同款平滑折线/面积图：蓝色面积线显示花费、橙色线显示归因销售额、绿色虚线显示 ROAS，并随每日、每周、每月尺度重新聚合。
- 新增当前周预算、区间花费、归因销售额和区间 ROAS 四张指标卡。
- 打开活动监控时在后台调用 Performance Campaign API 回读 `weeklyBudget` 和当前状态，成功时更新本地活动缓存并标记“Performance API 实时值”；网络或权限失败时显示“本地缓存”，不伪装成实时数据。
- TypeScript 类型检查、Vite 正式构建与 Rust `cargo check` 通过；Release EXE 重建成功，大小 25,252,864 字节，SHA-256 为 `1ECA6FA2F3F95883A016EF35C11F44F5DFE2576093E93243A349FE0927E4C83F`。

#### 同日修复：监控重复读取与周预算错误显示为 0

- 原因：监控窗口此前每次打开都会调用 Performance API；同时 `weeklyBudget` 在当前活动列表响应中是字符串，旧代码只用 `as_f64()` 接受 JSON 数字，解析失败后把默认值 0 写入并显示。
- 修复：预算解析兼容 JSON 数字、字符串及包含 `value`、`amount` 或 `budget` 的对象；新增 `budget_known` 与 `budget_updated_at` 数据库字段，未知预算显示 `—`，不再用 0 冒充。
- 缓存：前端会话和 SQLite 均缓存活动监控结果 15 分钟；有效期内重复双击直接显示缓存，不再访问 Performance API。过期后才后台回读，成功后更新缓存。
- 同步保护：常规 Performance 同步在响应缺少预算时保留已有有效预算，不再用缺省 0 覆盖；预算调整成功后立即写入已确认缓存。
- 验证：TypeScript 类型检查、Vite 正式构建与 Rust `cargo check` 通过；Release EXE 重建成功并实际运行 8 秒，数据库迁移未导致启动退出。大小 25,255,936 字节，SHA-256 为 `55550794C3E530C5FF0FB8E85A70E04F9B4BD5ADDFE2F9FFD97D29F8801A9918`。

#### 同日修复：Performance 周预算微卢布换算

- 原因：Performance API 的活动预算采用微卢布；后台 10,000 RUB 返回为 `10000000000`。上一版虽已兼容字符串解析，但未除以 1,000,000，因此显示为 10,000,000,000 RUB。
- 读取：`weeklyBudget / 1,000,000` 后以卢布显示和缓存。
- 写入：用户输入保持卢布口径，提交 PATCH 前乘以 1,000,000 转回 API 单位；回读后再次归一为卢布。
- 迁移：新增 `budget_scale_version`，现有已确认预算缓存仅执行一次除以 1,000,000 的迁移，防止程序每次启动重复换算。
- 验证：Rust `cargo check`、11 项单元测试、TypeScript 类型检查和 Vite 正式构建全部通过；Release EXE 重建后实际运行 8 秒，预算缓存迁移未引发启动错误。大小 25,260,032 字节，SHA-256 为 `40A96CD632D78014798BBAC293C021F16934A84B7F3F32B7293086E80DD9BB4D`。

### 更新记录模板

### 2026-08-26：竞品跟踪可核验监控升级（第一阶段）

- 依据：采用 `ecommerce-browser-monitoring` 的运行批次、稳定标识、受控状态、有限重试、来源证据和缺失值语义；不绕过登录、验证码或 Ozon 访问限制。
- 本阶段范围：新增商品快照采集批次、每个竞品的采集结果、状态、实际重试次数、来源 URL、页面环境、证据摘要和错误说明；批量采集页面展示成功、阻止、页面变化、不可访问及不完整数量。
- 状态口径：`ok` 表示稳定商品编号匹配且页面结构可解析；`blocked` 表示验证或访问限制；`changed_layout` 表示页面可访问但结构不可识别；`inaccessible` 表示无权访问或明确不可用；`ambiguous_match` 表示页面商品编号与目标不一致；`incomplete` 表示仅取得部分字段。
- 数据质量：价格和累计销量缺失保持 NULL，不使用 0；日、周、月销量仍为公开累计销量快照差，无法形成分母时显示“—”；保留页面原始文本用于复核。
- 调度与性能：仅对当天没有成功快照的活动竞品执行自动采集；临时网络失败最多有限重试，不无限循环；所有网络工作继续在后台线程执行。
- 安全边界：遇到验证码或验证页停止自动步骤，转入用户明确授权的浏览器验证/HTML 导入恢复流程，不读取 Cookie、密码或日常浏览器 Profile。
- 后续阶段：Ozon 关键词自然排名、广告排名和页内位置将作为独立页签与独立表实现，不与商品价格快照混表。
- 已实现：新增 `competitor_collection_runs` 和 `competitor_observations`，以 `run_id + competitor_id` 记录批次、状态、重试次数、来源、原始价格/销量文本、证据摘要与异常说明；原 `competitor_snapshots` 保持兼容，不破坏历史数据。
- 已实现：直连采集与验证后 HTML 导入均核对 Ozon 稳定商品编号；页面商品与目标不一致时记录 `ambiguous_match` 并禁止写入价格快照。
- 已实现：批量“立即采集全部”改为单一可核验运行批次；竞品页展示最近运行完成数、成功、不完整、验证阻止、页面变化、不可访问、身份冲突以及每个竞品的最新状态和备注。
- 已实现：每日自动采集仅跳过当天已有 `ok` 观测的竞品；`blocked`、`changed_layout`、`ambiguous_match` 或 `incomplete` 不再被错认为成功缓存。
- 修改文件：`desktop-next/src-tauri/src/lib.rs`、`desktop-next/src/OperationsPages.tsx`、`desktop-next/src/bridge.ts`、`desktop-next/src/types.ts`、`desktop-next/src/phase2.css`。
- 缓存与性能：查看竞品页只读取 SQLite；网络采集仍在 Tauri 后台阻塞任务中执行，不占用 React 界面线程；只对普通瞬时错误最多重试 1 次，验证和布局变化不重试。
- 测试和构建结果：Rust `cargo check --locked`、TypeScript `--noEmit`、Vite 正式构建已通过；新增缺失销量保持 NULL、验证页识别和商品编号冲突三项回归测试，Rust 共 14 项测试全部通过。Tauri Release EXE 已重建，实际启动 8 秒仍正常运行；大小 25,317,376 字节，SHA-256 为 `69ED8F805BBAA207BCBA886497657D1903542BEA6AFE9B41E9C2A01374E497E1`。
- 尚存风险/下一步：Ozon 公开商品页不保证始终提供累计销量，因此会保留 `incomplete` 而不伪造 0；下一阶段将实现关键词排名检查配置、搜索范围和排名历史看板。

#### 同日调整：竞品验证页全自动采集

- 用户反馈：“打开系统浏览器、手工另存 HTML、再导入”不属于目标自动化流程。
- 目标流程：直连成功则自动入库；直连受限、缺少必要字段或返回验证页时，程序自动启动可见的专用 Edge/Chrome，自动打开目标商品、等待页面稳定，自动校验商品编号并写入快照。
- 人工仅在 Ozon 真正显示验证码时完成平台验证；验证通过后程序自动继续，不再要求保存或导入 HTML。
- 安全边界：使用独立 `competitor_browser_profile`，不读取用户日常浏览器 Profile、Cookie 或密码；不绕过验证码或反自动化限制。
- 已实现：竞品采集的第 1 阶段尝试 35 秒后台直连；直连失败、验证受阻或价格/累计销量不完整时，第 2 阶段自动启动专用可见浏览器，最长等待 240 秒。
- 已实现：专用浏览器每秒读取当前 DOM，自动识别验证页，要求最终 URL 商品编号与目标一致，且解析结果连续稳定后才写入快照；商品跳转错位时继续拒绝写入。
- 界面调整：删除竞品页的 HTML 路径输入、“另存网页”与手工导入按钮；单品“自动验证采集”、添加后采集、采集全部和每日自动采集共用同一条自动流程。
- 状态与证据：直连和专用浏览器分别记录 `ozon_public_page` / `dedicated_browser`，最终超时记录 `blocked`，不把验证页或空页写成商品数据。
- 测试与构建：Rust `cargo check --locked`、14 项 Rust 测试、TypeScript `--noEmit`、Vite 正式构建及 Tauri Release 构建全部通过。EXE 实际启动 8 秒未异常退出，大小 25,331,712 字节，SHA-256 为 `7B9EED9A9651D896E50A740089207B26D8F97804D17692A307B9B01400A6E53A`。
- 验证边界：本轮已完成编译、回归和启动验证；真实 Ozon 验证码仍由平台按当时风控决定，应在竞品页选择一个实际商品执行端到端采集确认；程序不尝试绕过验证。

#### 同日修复：专用浏览器只显示 Edge 新建标签页

- 原因：Edge 启动时已自带一个前台“新建标签页”，程序又通过 CDP 创建了第二个后台标签并对其导航，因此用户看到的前台页一直是 Edge 首页。
- 修复：不再新建第二个标签；等待并复用 Edge 初始标签，将竞品 URL 直接导航到该标签，然后显式调用 `bring_to_front` 保证竞品页位于前台。
- 验证：14 项 Rust 测试、TypeScript 类型检查、Vite 正式构建和 Tauri Release 构建全部通过。新 EXE 大小 25,343,488 字节，SHA-256 为 `E1752E9924E7CB84F4D07AAE2497A251D15F84CFC64FCDB81153849AE924A792`。

#### 同日继续：对照旧跟卖软件的采集队列、进度与停止模型

- 参考源：`D:\OZON 跟卖软件\新建文件夹\新建文件夹 (2)\RFBS上品工具\core.py` 的 Playwright 持久会话/首页 `goto`，以及 `app.py` 的队列、阶段进度、`cancel_requested` 和安全检查点。
- 已确认问题：当前 CDP 初始标签仍可能是 Edge 的特殊 NTP 页，对该对象执行导航未必改变用户可见页。修复方向改为显式创建 Ozon 目标、强制置前、校验实际 URL，并关闭/忽略 NTP 标签。
- 任务管理目标：后端持有单一竞品采集队列，提供运行中、总任务、已完成、成功、失败、当前商品、当前阶段和停止请求。停止不强行杀死数据库写入，在浏览器轮询或单个任务边界安全退出。
- 已实现导航修复：改回显式创建普通 CDP 页目标，导航后强制 `bring_to_front`；20 秒内持续核对实际 URL，未进入 `ozon.ru` 时重新导航，最终仍失败则记录当前 URL 并终止，不在 Edge 首页空等。
- 已实现任务控制：新增 `start_competitors_collection`、`competitor_collection_progress`、`stop_competitors_collection` 后端命令；采集全部改为立即返回的后台队列，避免界面等待整个批次。
- 已实现可视进度：React 每 700 毫秒读取一次状态，显示总任务、已完成、成功、失败、当前商品和当前阶段（排队/后台直连/专用浏览器/写入/停止/完成），并提供进度条。
- 已实现停止：“停止采集”立即设置原子停止信号；导航检查、浏览器 DOM 轮询与单品边界均检查该信号，安全结束当前步骤并将运行记录标记为 `stopped`。
- 自动日采集一致性：竞品页打开时的“今日待采集”也接入同一进度和停止状态；它与手动“采集全部”互斥，不会同时启动两个浏览器采集批次。
- 验证结果：Rust `cargo check --locked`、14 项 Rust 回归测试、TypeScript `--noEmit`、Vite 正式构建与 Tauri Release 构建全部通过。最终 EXE 大小 25,356,288 字节，SHA-256 为 `9EF1EEDDD19914A3D8B5281200CB8A9C4B3BFFEBB92B1A6084769E8AF4DACCF3`。
- 实机边界：当前环境未通过自动化测试凭空添加或改写用户竞品数据；真实 Ozon 端到端结果应以软件内现有竞品队列执行结果为准。新的 URL 校验会在失败时直接显示实际停留地址，便于下一步精确诊断。

#### 同日继续：竞品逐任务控制面板与浏览器直达修复

- 已定位截图中 Edge 仍停留在 MSN 新标签页的原因：只通过 CDP 创建或导航页面目标，不能保证 Edge 的可见首标签就是该目标。现在将竞品完整 URL 作为 Edge/Chrome 的进程启动参数，首个可见标签直接进入目标 Ozon 商品页，再用稳定商品 ID 校验最终页面。
- 未增加验证码绕过、Cookie 注入或反自动化规避。若 Ozon 要求验证，任务进入浏览器等待阶段；用户完成平台验证后软件自动继续。有限等待超时记录 `blocked`，缺失字段保持为空而不是写入 0。
- 参考旧项目 `RFBS上品工具/app.py` 的任务队列与安全停止检查点，将采集批次展开为逐条任务：每项展示序号、商品 ID、完整来源 URL、状态、当前阶段、说明、重试次数及起止时间。
- 单任务状态包含等待、采集中、停止中、已停止、成功、不完整和失败；每项均可单独停止，停止一个不会影响其他任务。正在执行的任务在网络或浏览器轮询安全检查点退出，未执行任务直接跳过。
- 保留“停止全部”，同时把正在执行和未执行任务分别标记为停止中、已停止。
- 前端任务区重构为带状态色、滚动明细和响应式布局的任务面板，解决只显示总数、无法判断和控制单条任务的问题。
- 验证结果：Rust `cargo check --locked` 通过；Rust 14 项测试全部通过；TypeScript `tsc --noEmit` 通过；Vite 生产构建及 Rust Release 构建通过。新 EXE 大小 25,074,176 字节，SHA-256 为 `72F84CFF18F93B688B9FE2DE66E5C39D293F9FE3125A0E036F07A559747041C7`。

#### 同日修复：双击后 Edge 显示 localhost 拒绝连接

- 现象属于旧入口：截图为系统 Edge 的 `ERR_CONNECTION_REFUSED`，说明被点击的文件仍在尝试打开本地网页服务；正式 Tauri 客户端使用内嵌 WebView，不会把主界面打开到 Edge，也不依赖 localhost。
- 核查发现父目录同时存在 `.pnpm-store` 内 2026-08-25 的旧同名构建缓存，以及仓库中的旧 Python 构建脚本，容易误点旧文件或旧快捷方式。
- 已把本轮正式 Tauri Release 固定复制到项目根目录 `ozon-analytics-next.exe`，以后可直接双击该文件，不需要 CMD、Python、npm 或本地服务。
- 实测根目录 EXE 启动后持续运行 8 秒且未异常退出；大小 25,074,176 字节，SHA-256 为 `72F84CFF18F93B688B9FE2DE66E5C39D293F9FE3125A0E036F07A559747041C7`。

#### 同日二次修复：Release 窗口仍指向 localhost

- 用户复测证明窗口标题虽为 Ozon ERP，但内嵌 WebView 仍显示 `localhost ERR_CONNECTION_REFUSED`。最终根因是上一轮直接执行了 `cargo build --release`：该命令没有经过 Tauri CLI 的发布上下文，未启用内嵌资源使用的 `custom-protocol`，所以编译产物仍读取 `devUrl=http://localhost:1420`。
- 已改用项目唯一正确的发布流程 `desktop-next/scripts/build-tauri-release.cmd`：先执行前端生产构建，再由 Tauri CLI 使用发布协议编译 `--no-bundle` EXE。
- 新产物已覆盖项目根目录 `ozon-analytics-next.exe`。实际启动后用 Windows `PrintWindow` 捕获应用窗口，确认显示 ERP 经营总览、本地店铺数据和图表，不再出现 localhost 页面。
- 正确 EXE 大小 25,332,224 字节，SHA-256 为 `E4E29603E18D2C1B1A9630AB2667FEAC8954A6EAFC70F38007655CA8ED4B50A5`。
- 后续约束：正式交付不得再用裸 `cargo build --release`；必须运行 Tauri CLI 构建脚本。裸 Cargo 构建仅允许用于编译/测试验证，不得复制为用户发布 EXE。

#### 同日调整：普通浏览器可访问、自动化 Edge 返回 Ozon 断网页

- 用户用同一商品 `2578856473` 证明普通 Chrome 与手工 Edge 均可正常访问，因此排除商品 URL、DNS和整机网络故障；此前“系统无法直连”的结论修正为“自动化独立浏览器会话受阻”。
- Ozon 返回带事件编号的俄文“似乎没有连接”页面，而地址栏仍是正确商品 URL；这是该独立会话的受阻响应，不能当作商品下架或真实销售数据。
- 合规调整：竞品采集优先使用已安装的 Google Chrome，Edge 仅作为回退；Chrome 与 Edge 使用各自独立 Profile，避免两种浏览器共用同一缓存；恢复自动化库默认关闭的后台页面网络和旧 NetworkService 强制参数，但保留明确的自动化标记，不加入隐藏自动化或绕过验证码的参数。
- 验证：Rust `cargo check --locked` 通过，14 项 Rust 测试通过，前端生产构建与 Tauri Release 构建通过。
- 正在运行的根目录 EXE占用文件，无法在线覆盖，因此本轮候选发布为 `ozon-analytics-next-chrome.exe`，大小 25,333,248 字节，SHA-256 为 `9B3C011B516B312C5615AC11246553DDE0713A55851361EA0DA539BFD68E5372`。关闭旧 Ozon ERP 后可将其覆盖回正式文件名。


### 2026-08-26：修复月度缺成本编辑器构建错误并重新构建

- 目的：修复 GitHub 最新提交中月度缺成本编辑器条件渲染缺少结束大括号导致的 TypeScript `TS1005` 错误，恢复可构建状态。
- 修改模块/文件：`desktop-next/src/OperationsPages.tsx`。
- 数据口径或数据库变化：无；仅修复 JSX 语法，不改变成本保存和月度盈亏重算逻辑。
- 缓存与性能影响：无。
- 测试和构建结果：TypeScript `--noEmit`、Vite 正式构建通过；Rust 11 项单元测试全部通过；Tauri Release 构建通过。新 EXE 为 23.81 MB，直接启动后持续运行 8 秒且未立即退出，SHA-256 为 `923615C9664C8459DA7623BD6FAFC32B539514397BF9F2A2A76DC0544AC9E69D`。
- 尚存风险/下一步：继续完成 RFBS 属性、校验、正式发布、轮询和台账回写链路。

### 2026-08-26：统一新旧月度利润口径并修复 Finance 重复累加

- 目的：新版本同时明确展示税前利润和税后利润，并复核 2026 年 7 月旧版与新版月度盈亏差异。
- 修改模块/文件：`desktop-next/src/OperationsPages.tsx`、`desktop-next/src-tauri/src/lib.rs`。
- 核对结论：旧版税前利润公式为“Finance/应计净额 − 已交付商品采购成本 − 已交付商品头程”；税后利润再扣“销售额 × 税率”和“正向 Finance 净额 × 回款手续费率”。新版原先把下单件数作为成本件数，包含尚未交付或退货订单，和旧版按已交付件数计成本的口径不一致。
- 界面变化：月度盈亏第一张利润卡改为“税前利润”，第三张保留“税后利润”，并直接显示税前计算口径，避免把 Finance 结算净额误认为利润。
- 数据逻辑变化：采购与头程优先使用同 SKU 的已交付数量；没有交付快照时才回退到订单量。业务报表缓存版本升级为 `finance-v3-delivered-cost-units`。
- Finance 根因与修复：部分 `/v3/finance/transaction/list` 记录没有稳定 `operation_id`，旧逻辑以“响应序号+日期”生成主键；分页排序改变后重复同步会把同一笔交易再次插入，导致 Finance 净额及利润随同步次数增长。新逻辑先完整获取全部 API 分页，成功后在同一数据库事务内替换所选日期区间，并清除业务报表明细缓存；获取失败不会删除旧数据，重复成功同步也不会叠加。
- 截图对账：旧版税前利润约 ₽1,039,853；截图中的新版约 ₽899,885.99。两版截图并非同一 Finance 缓存快照（旧版显示 22,879 笔，新版期间构成显示 25,989 笔），同时成本数量口径不同，因此不能只用最终利润反推单一差额。修复后必须对同一店铺、同一月份重新同步 Finance，再以相同已交付成本口径对账。
- 数据修复方式：升级后点击该月“同步应计费用”，成功响应会安全替换该月旧 Finance 缓存并重算；不直接离线猜测、删除或合并历史流水，避免误删金额相同但真实存在的不同交易。
- 缓存与性能影响：同步期间 API 请求仍在数据库事务之前；只有全部读取成功后才短暂执行区间替换，页面后台任务机制不变。
- 测试和构建结果：TypeScript/Vite 正式构建与 Tauri Release 编译通过；仅保留既有 `headless_chrome::wait_for_initial_tab` 弃用警告。根目录 EXE 已更新，大小 25,338,368 字节，SHA-256 为 `8CC46279611B11502AC6A5EB6357681BB5E77BBFF2764C82D7898E15F1A549F0`。
- 尚存风险/下一步：Ozon 后台后续若提供永久稳定的流水 ID，应优先保存平台 ID；当前通过成功后的区间原子替换保证幂等。

### 2026-08-26：修复竞品正常商品页误判、可见价格和主图采集

- 目的：正常 Ozon 商品页右上角已显示价格时必须采集售价，同时采集商品主图；公开页面不提供销量时保留空值并允许用户手工填写。
- 修改模块/文件：`desktop-next/src-tauri/src/lib.rs`、`desktop-next/src/OperationsPages.tsx`、`desktop-next/src/bridge.ts`、`desktop-next/src/phase2.css`。
- 根因：旧判断对整份 HTML（包括 Ozon 自身 JavaScript）搜索 `captcha/проверка`，正常页面脚本包含这些单词时会误判为 `Antibot Challenge Page`；旧解析器只读取旧版 JSON 的 `price/cardPrice` 和 `og:image`，没有读取当前 SPA 已渲染的价格组件与商品图库。
- 采集变化：验证限制只根据明确挑战页标题及可见提示判断；浏览器从可见价格组件读取第一个卢布价格，并优先从商品图库中选择尺寸足够的主图，随 HTML 一起校验和入库。支持带普通空格、不换行空格的价格（如 `1 686 ₽`）。
- 销量口径：累计销量不再是采集成功的必要字段。价格和主图齐全即标记公开页面采集成功；销量为空不会写成 0。每张竞品卡新增“手填当前累计销量”和“保存销量快照”，日/周/月销量继续使用相邻累计快照差计算。
- 测试和构建结果：新增可见价格 `1 686 ₽`、主图及空销量解析回归用例；TypeScript/Vite 与 Tauri Release 编译通过。正式 EXE 正在运行并占用文件，因此输出候选 `ozon-analytics-next-competitor-fix.exe`，大小 25,329,152 字节，SHA-256 为 `B94236102C561B3A6ADDE70776F7068BD5E4DDA4050234E07326EA3C86DB4D94`。
- 尚存风险/下一步：关闭旧 Ozon ERP 后，将候选 EXE 改名覆盖 `ozon-analytics-next.exe`；之后对原先误判的竞品重新执行采集快照。

### 2026-08-26：二次审计 Ozon 应计费用总账与分类

- 目的：针对新版、旧版与 Ozon 后台应计费用相差过大的问题，直接审计 2026-07 原始 Finance API 表，而不是用利润结果倒推。
- 审计证据：当前店铺数据库 `shop_9ce5632eec80.db` 中 2026-07 共 22,879 笔 Finance 原始记录，`COUNT(DISTINCT raw_json)` 同为 22,879；`SUM(amount)=₽3,345,905.85`、`SUM(accruals_for_sale)=₽10,831,284.33`，分别与后台总计 ₽3,345,906、销售和退货 ₽10,831,284 一致。
- 截图差异根因：出错截图显示 48,868 笔，销售/退货 ₽23,141,883.66、应计费用 -₽16,294,655.90，是修复前旧进程/旧报表快照重复累计的结果；不是 API 当前返回值，也与采购成本填写无关。
- 分类逻辑问题：新版分类卡原先仅累计 `services[]`，遗漏直接记在 operation `amount` 上的 CPC/CPO、外部推广、提前回款和 Premium 等费用；因此即使总账恒等式正确，广告等分类仍会与后台不一致。
- 分类修复：每笔交易使用 `amount = accruals_for_sale + sale_commission + services + residual`，将 residual 按 `operation_type + operation_type_name` 归类。`StarsMembership` 不再误归广告；CPC、按订单推广、外部推广、评价推广、Premium、subscription、cashback 纳入后台“推广和广告”口径。
- 复核结果：代理佣金为 -₽4,781,717.35，对应后台 -₽4,781,717；推广和广告为约 -₽981,325.49，对应后台 -₽981,325；应计费用总额继续以 Finance 恒等式计算为 -₽7,503,247.24，对应后台 -₽7,503,247。
- 修改模块/文件：`desktop-next/src-tauri/src/lib.rs`；沿用上一轮成功后按日期区间原子替换 Finance 缓存的幂等同步修复。
- 测试和构建结果：TypeScript/Vite 与 Tauri Release 编译通过。候选程序为 `ozon-analytics-next-finance-audit-fix.exe`，大小 25,331,200 字节，SHA-256 为 `258E1C45E2F63F85735E8FCD5EE309AF44A6C82B2E7F2C89E84713D48E55700D`。
- 操作要求：必须关闭仍占用旧正式 EXE 的 Ozon ERP，再启动本候选版本；对 2026-07 执行一次“同步应计费用”。成功后页面应显示 22,879 笔，而不是 48,868 笔。

### 2026-08-26：跨境店 2026-08-20 至 2026-08-26 新旧利润差异审计

- 目的：解释旧版跨境实时预估利润 ¥2,856 与新版无法给出完整利润的原因，并区分销售、广告、成本、费率及 Finance 对账口径。
- 数据完整性：跨境店 Finance 当前共 8,503 笔，`COUNT(DISTINCT raw_json)` 同为 8,503，不存在本土店此前的重复累计问题；缓存日期为 2026-05-28 至 2026-08-25。
- 金额桥接：旧版销售约 ¥18,495、广告约 ¥2,158、平台费约 -¥2,387、采购和运费约 -¥11,094，利润约 ¥2,856。新版销售 ¥18,494.65、广告 ¥2,157.70、平台费 -¥2,476.37、采购和运费 ¥11,076.21；若暂不执行缺成本保护，算术结果约 ¥2,784.37，与旧版相差约 ¥71.63，而不是数量级错误。
- 费率差异：旧截图使用佣金 11.57% / 收单 1.61%（样本截至 2026-08-19）；当前 API 历史缓存按完整结算样本计算为佣金约 11.61% / 收单约 1.89%。平台费差约 ¥89，是两版利润差的主要来源。跨境实时利润采用历史已结算费率估算，不等于选定下单日期内的最终 Finance 结算。
- 完整性阻断：当前期间新增 SKU `5302459703`，销量 1 件、销售额约 ¥34.58、采购成本 ¥6，但 `weight_kg` 为空，因此无法套用跨境定价参考运费公式。新版选择显示“成本/历史费率未完整”，避免把未知运费当作 0；旧截图生成时显示 26 个出单 SKU、0 个缺成本，数据快照不同。
- Finance 对账列：按财务发生日期汇总，销售额按下单日期汇总；单 SKU 与期间总计不能直接作为实时利润公式的收入替代项。它只用于结算对账。
- 修复建议：在商品成本中为 SKU `5302459703` 填写真实重量后重新打开报表；不要为了得到利润数值而把缺失重量默认为 0。若要完全复现旧版金额，还必须固定相同的 Finance 历史快照和费率样本截止日。
- 代码变化：无。本条为数据和口径审计记录。

### 2026-08-26：竞品采集迁移检查点（旧版 Python/Playwright 接入中）

- 目的：按用户指定，复用 `D:\OZON 跟卖软件\新建文件夹\新建文件夹 (2)\RFBS上品工具\core.py` 的持久化浏览器、页面稳定等待、商品 ID 校验和图库筛选思路，替换新版中不稳定的纯 Rust 浏览器采集链路。
- 当前节点：新增根目录 `competitor_collector.py`，并已打包出随发布包携带的 `competitor-collector.exe`；Rust 已新增 `collect_competitor_python_html`，负责启动独立采集进程、轮询停止信号、超时终止、读取 JSON、校验商品 ID，并把价格和主图交给现有 SQLite 快照逻辑。下一步从 `refresh_competitor_for_run` 的浏览器回退分支继续验证。
- 保留的旧版逻辑：优先 Edge、Chrome 回退；独立持久化 profile；真实可见页面；DOM 稳定两轮后返回；右上角卢布价格；Ozon CDN 商品主图；最终 URL 商品号一致性检查。
- 未迁移内容：旧版参数 `--disable-blink-features=AutomationControlled` 会隐藏自动化特征，因此未复制；不导入 Cookie、不绕过验证码或平台访问限制。公开页面没有可信累计销量时仍返回空值，允许 ERP 手工保存销量快照。
- 本轮实测：普通 Edge/Chrome 已能够自动打开商品 `2578856473` 的真实 Ozon 页面，页面显示售价 `1666₽` 和主图；独立 Python 源码第一次运行遇到页面导航导致执行上下文销毁，已增加导航期间重试。随后一次测试被任务中断，尚未取得“Python JSON 成功 → Rust 写入 SQLite”的完整证据。
- 当前状态：**未完成、可继续**。GitHub 本次提交是明确的迁移检查点，不代表竞品采集已验收成功。
- 下一步：重新打包最新 `competitor-collector.exe`，单独运行确认返回 `ok=true/name/price/image`；再构建 Tauri Release，执行 ERP 单任务采集并查询 `competitor_snapshots`；成功后补充截图、EXE 哈希和最终验收记录。

### 2026-08-26：复用旧版“跟卖模式”商品采集逻辑

- 已核实外部旧源码：跟卖自动任务第 1 阶段调用 `scrape_reference`；直连缺少完整图库时自动进入 `scrape_reference_browser`，使用持久化 Edge/Chrome 会话等待验证页结束，并校验最终 URL 的 Артикул，防止采集到错误变体。
- `competitor_collector.py` 已复用旧版的固定左侧缩略图轨道识别、中央预览图排除、`wc2000` 高清地址转换、图库 URL 去重、页面水合及稳定轮询；同时保留竞品模块的可见售价读取和销量空值纪律。
- Python 采集器现在返回主图、完整图库、图库识别模式和来源；Rust 桥接将这些字段写入采集证据，并继续把主图、售价交给 `competitor_products` / `competitor_snapshots`。
- 新增 `scripts/build-competitor-collector.ps1`，用隔离 Python 3.12 打包采集器，并在 Release 目录存在时自动复制到主程序旁。
- 本节点的静态迁移和采集器打包已完成；仍需使用真实 Ozon 商品执行 ERP 单任务采集，并查询 SQLite 快照完成线上页面验收。

### 2026-08-26：月度盈亏采购成本口径复核

- 2026 年 7 月当前缓存有 6,718 件下单量，但 `delivery_events` 与 Seller Analytics 的妥投字段均为空；旧实现静默回退到全部下单量，按 `采购成本 CNY × 14 RUB/CNY × 6,718 件` 核算，得到截图中的 ₽2,134,538.56。
- Finance 中同期有 5,902 笔可精确归属 SKU 的 `OperationAgentDeliveredToCustomer`。按相同成本重算为 ₽1,866,705.96；旧口径多计 ₽267,832.60。
- 新版月度盈亏的成本件数优先级改为：Posting 交付事件 → Finance 已交付流水 → 0，不再把取消、退货或尚未履约的全部下单件数直接计入采购成本，并提升缓存指纹版本使旧报表缓存自动失效。旧 Python 的日常经营预估仍按下单量估算，避免把“经营预估”和“已结算盈亏”错误合并。

### 2026-08-26：新旧版本数据库彻底隔离

- 问题确认：新版原 `locate_data_dir` 会沿 EXE 祖先目录查找 `data/shops.json`，因此与旧 Python 共用 `data/ozon_analytics.db`，存在运行时串库风险。
- 新版唯一数据根目录改为 `data-next/`；默认本土店使用 `ozon_next_default.db`，其他店铺使用 `shops/shop_next_<店铺ID>.db`。WB、浏览器会话、竞品和上品运行数据也全部落在新版根目录下。
- 首次启动若只有旧 `data/`，使用 SQLite `VACUUM INTO` 建立一致性快照，只迁移一次；新注册表写入完成后，后续启动只识别带 `database-generation.json` 的 `data-next/`，不再打开旧数据库。
- 一次性迁移保留 API 配置、成本资料和历史业务数据，但清空月度报表缓存、分析缓存和同步断点，防止继承旧版计算结果。旧数据库不删除、不覆盖，可作为只读回退备份。
- Rust Release 已使用项目 `.cargo/config.toml` 的 rsproxy 稀疏镜像离线缓存完成构建；竞品采集测试 4/4 通过。首次直接执行 `cargo build --release` 会遗漏 Tauri 的 `beforeBuildCommand`，导致 EXE 仍访问 `localhost:1420`；已改用唯一正确入口 `desktop-next/scripts/build-tauri-release.cmd`，先生成 Vite `dist` 再嵌入静态资源。修复后 `ozon-analytics-next.exe` SHA-256 为 `795844FA5F4D557389FDF35BFCF839C8F3C25BE8052C245EACB3DE5491BDA217`，配套 `competitor-collector.exe` SHA-256 为 `3541ABB3F99059A6A9BD5316CA74CA6D71698411A72AC48FB1DDEBE064F6C728`。
- Release 启动验收：生产构建已嵌入 `index-Bj_BlHo2.js`、`react-vendor-B1co94AF.js` 等资源；错误进程已关闭，交付 EXE 保留在 `desktop-next/src-tauri/target/release/`。
- 相关文件：`competitor_collector.py`、`desktop-next/src-tauri/src/lib.rs`、竞品任务/进度前端文件及本 Markdown。

### 2026-08-26：月度盈亏成本完整性与全卡片明细

- 根因：2026-07 的已交付成本口径中有 2 件商品缺少头程成本，分别属于 SKU `2914266239` 和 `3450387569`；税前利润因此显示“成本未完整”。原“缺成本 SKU”卡片却仍按下单销量查询，错误显示为 0，造成同一页面口径矛盾。
- 数据口径修复：月报、缺成本 SKU 数量和缺成本明细统一使用“Posting 已交付事件优先、Finance `OperationAgentDeliveredToCustomer` 兜底”的已交付件数。7 月应显示缺成本 2 件、2 个 SKU；补齐两项头程成本后，税前及税后利润自动恢复计算。
- 交互变化：月度盈亏顶部税前利润、Finance 净额、税后利润以及“期间构成”的全部汇总卡均可点击。Finance 分类继续展示逐项 API/服务明细；销售、订单、广告展示逐日明细；成本、税费、利润、归属和对账卡展示计算组成、数据来源与口径说明。
- 缓存：报表指纹提升为 `finance-v5-consistent-missing-cost`，旧的矛盾缓存会自动失效，无需删除业务数据。
- 修改模块/文件：`desktop-next/src-tauri/src/lib.rs`、`desktop-next/src/OperationsPages.tsx`、本 Markdown。
- 验证：TypeScript/Vite 正式构建通过；Rust `cargo check --offline` 通过。SQLite 对 2026-07 的同口径复核结果为缺成本 2 件 / 2 个 SKU。

### 2026-08-26：竞品采集器 Windows 输出编码修复

- 错误现象：竞品任务进入 `incomplete`，提示 `stream did not contain valid UTF-8`；失败发生在 Rust 读取采集器标准输出阶段，尚未进入商品链接、价格或图片校验。
- 根因：PyInstaller 采集器继承 Windows 控制台代码页，`ensure_ascii=False` 输出的商品名称或错误信息可能成为本地编码字节，而 Rust `read_to_string` 要求严格 UTF-8。
- 修复：采集器 IPC 改为 ASCII-only JSON（Unicode 使用 JSON 转义，解析后内容不丢失）；Rust 改为字节读取、兼容旧采集器的有损解码，并从输出中提取 JSON 对象，因此升级过程中旧、新 sidecar 都不会再触发 UTF-8 边界错误。
- 修改模块/文件：`competitor_collector.py`、`desktop-next/src-tauri/src/lib.rs`、本 Markdown。
- 验证：采集器单元测试 2/2 通过；`competitor-collector.exe` 已重新打包并复制到 Release 目录；Tauri Release 构建通过。

### 2026-08-26：月度盈亏全公式复核与配送明细统一

- 差异根因：配送卡片将常规配送与末公里配送合并为“配送费用”，但点击明细仅过滤 `delivery`，遗漏内部分类键 `last_mile`。因此卡片为 ₽-927,059.03，明细当时只显示 ₽-850,903.28。
- 配送复算：常规配送 `MarketplaceServiceItemDirectFlowLogistic` 为 6,781 条 / ₽-850,903.28；末公里 `MarketplaceServiceItemRedistributionLastMileCourier` 与 `MarketplaceServiceItemDeliveryToHandoverPlaceOzon` 合计 6,504 条 / ₽-76,155.75；两者合计 ₽-927,059.03，与卡片完全一致。
- Finance 全分类复核：销售/退货 ₽10,831,284.33；佣金 ₽-4,781,717.35；广告 ₽-981,325.49；配送 ₽-927,059.03；退货物流 ₽-86,122.84；收单 ₽-431,148.84；仓储 ₽-118,823.44；罚款 ₽-11,489.68；其他 ₽-147,691.81。22,879 条原始记录按 `amount` 汇总为 ₽3,345,905.85；逐项恒等式误差仅约 ₽0.00000007（浮点舍入）。
- 利润公式复核：采购成本 ₽1,866,705.96、头程 ₽585,137.73；已结算税前利润 = Finance 净额 − 采购 − 头程 = ₽894,062.16；税额 = 销售额 ₽12,570,917 × 3% = ₽-377,127.51；提现费 = Finance 净额 × 10% = ₽-334,590.59；税后利润约 ₽182,344.07。
- 修复：末公里和交接点服务统一使用 `delivery` 分类键，配送卡片和点击明细从源头采用同一分类；新增配送、退货、收单、仓储及广告分类回归测试。
- 修改模块/文件：`desktop-next/src-tauri/src/lib.rs`、本 Markdown。测试结果：Finance 分类测试 2/2 通过。

```markdown
### YYYY-MM-DD：更新标题

- 目的：
- 修改模块/文件：
- 数据口径或数据库变化：
- 缓存与性能影响：
- 测试和构建结果：
- 尚存风险/下一步：
```

### 2026-08-27：同步 GitHub 正式版 1.0.0 并重建启动器

- 目的：从 `https://github.com/heibaiyuanfen/ozon` 获取后续更新，重新构建桌面启动器和 Windows 安装程序。
- Git 节点：本地 `main` 已从 `7d811bb` 快进至 `da5770c`；包含 `2e32e31 fix: restore compatibility after upstream update` 与 `da5770c release: Ozon Analytics desktop 1.0.0`。拉取前的临时 Rust 调试修改单独保存在 `stash@{0}`（`local pre-da5770c compile fix`），未覆盖正式版源码。
- 修改模块/文件：本次主要接收远端 28 个文件的更新，包括新版本数据隔离、竞品 Python/Playwright 采集器及构建脚本、经营增长/商品差异化/跨境运营页面、Finance 与月度盈亏修复；本地仅追加本条迁移记录，没有改写业务数据。
- 数据口径或数据库变化：沿用正式版 `data-next/` 独立数据库方案；原 `data/`、`node_modules/` 与用户运行数据均保留。首次运行时按正式版迁移规则建立新版数据库快照。
- 测试和构建结果：`tests/test_competitor_collector.py` 2/2 通过；Vite 正式前端构建通过；Tauri 1.0.0 Release 构建通过；NSIS 安装包构建通过。根目录启动器已更新为本次构建，SHA-256 `9AF05A54EE68D17BB82A06215673E8EB00B19B07F1B2E51D8647305E4E330EC7`；配套采集器 SHA-256 `C54516F33BB80BEEE94EE419032B351AE5CCAEA1AA841F81F87864C28D3821BE`；安装包 SHA-256 `AF2E5C1359BC433746B9842B163BA9127E5903928A08DA6820038179569D6C1D`。
- 交付位置：根目录 `ozon-analytics-next.exe`；采集器 `competitor-collector.exe`；安装包 `desktop-next/src-tauri/target/release/bundle/nsis/Ozon Analytics_1.0.0_x64-setup.exe`。
- 尚存风险/下一步：当前机器未安装 `uv`，所以本轮没有从 Python 源码重新打包采集器，而是继续配套仓库现有且已通过 IPC 修复构建的采集器。后续修改 `competitor_collector.py` 后，必须先运行 `scripts/build-competitor-collector.ps1` 再发布。

### 2026-08-27：清理重复构建产物并完成旧库到新库迁移核验

- 目的：缩减项目体积，同时保留源码、开发依赖、业务数据、正式程序和可安装发布包；确认旧版 `data/` 的数据完整进入新版 `data-next/`。
- 清理范围：删除可重新生成的 Rust `desktop-next/src-tauri/target/`（其中 Debug 约 6.23 GB、Release 中间产物约 2.45 GB）、PyInstaller `build/`/`dist/`、Python 缓存、6 张运行诊断截图、10 个历史候选启动器，以及旧竞品采集测试产生的 10 组临时浏览器 profile。正式安装包已先复制到根目录 `release/`，没有随 Target 缓存删除。
- 保留内容：`desktop-next/node_modules/`、所有源码和锁文件、旧版 `data/` 业务数据库、新版 `data-next/` 业务数据库、根目录正式 `ozon-analytics-next.exe`、`competitor-collector.exe` 和 `release/Ozon Analytics_1.0.0_x64-setup.exe`。
- 数据迁移结果：正式版在 2026-08-27 首次启动时通过 SQLite `VACUUM INTO` 将 4 个店铺数据库和 WB 数据库建立到 `data-next/`，并生成 `database-generation.json`。逐店核对后，旧库与新库共有业务表的行数一致；差异仅为设计上主动清空的 `business_report_cache`、`analytics_detail_cache`、`sync_progress`，以及新版自动新增的扩展表 `competitor_manual_metrics`。
- 店铺映射：`data/ozon_analytics.db` → `data-next/ozon_next_default.db`；三个 `data/shops/shop_<id>.db` → 对应的 `data-next/shops/shop_next_<id>.db`。旧库仍完整保留，作为只读回退副本，不再被新版运行时打开。
- 体积变化：项目约从 10 GB 降至约 552 MB；其中旧数据约 141 MB、新数据约 140 MB、`node_modules` 约 193 MB。后续构建会重新生成 `target/`，完成发布后可再次安全清理。
- 尚存风险/下一步：不要手工把两个数据库目录混合使用。新增业务数据只写入 `data-next/`；如需重新执行全量迁移，应先关闭程序、备份两个目录，并明确采用覆盖还是增量合并策略。

### 2026-08-27：竞品逐任务手动采集与库存—约仓集群联动

- 目的：根据用户提供的 `新建文件夹(2).rar` 和旧版 `RFBS上品工具` 源码，恢复“任务由用户明确启动、每项可单独停止”的采集控制；迁移旧版库存集群配送量与 FBO 约仓计划的联动。
- 旧版逻辑核对：竞品能力来自 `app.py` 的任务队列/协作式取消和 `core.py` 的 `scrape_reference` 持久浏览器、页面稳定等待、商品身份校验与图库水合。新版保留可核验来源、时间、状态、重试和证据字段；验证码、访问限制或不公开销量仍标记为受阻/空值，不绕过限制或使用 AI 猜测。
- 竞品变化：打开竞品模块只读取本地任务和最近进度，不再启动采集，也不再每小时自动采集；添加链接只创建待采集任务。后端新增 `start_competitor_collection_task`，每张卡片提供“开始采集/停止采集/停止中”，原全量运行仍需用户主动点击。采集器移除隐藏自动化特征的浏览器参数。
- 约仓变化：约仓页新增库存集群计划表，按 SKU/货号/商品/集群检索，展示可售、在途、已申请、集群日均销量、目标天数建议量和实际约仓数量；保存到共享 `replenishment_plan` 后，库存管理与约仓读取同一条 `SKU + macrolocal_cluster_id` 计划记录。
- 模式区分：`CROSSDOCK/越库` 明确要求选择 Ozon 越库发货点，由平台转运到目的仓；`DIRECT/直送` 直接送往草稿计算出的目的仓，不选择越库发货点。本节点完成数量计划与模式选择迁移；真实创建草稿、选择目的仓和预约时段仍沿用严格确认、写操作不自动重试的安全边界。
- 修改模块/文件：`competitor_collector.py`、`desktop-next/src-tauri/src/lib.rs`、`desktop-next/src/bridge.ts`、`desktop-next/src/types.ts`、`desktop-next/src/OperationsPages.tsx`、本 Markdown。
- 测试和构建结果：TypeScript/Vite 正式构建通过；竞品采集器单元测试 2/2 通过；Rust `cargo check` 与 Tauri Release 构建通过（仅保留既有未使用/弃用警告）。根目录启动器已更新，SHA-256 为 `FF35FAD391F69D1754FB4901905C4B17928D32117535B05976D6ACE91AA72612`。实际启动后确认“约仓计划”页面正常渲染，真实显示库存集群、已保存配送量、越库/直送说明及现有供应单。
- 清理：用于阅读 RAR 的 `.legacy-review-20260827/` 和约 4 GB 可再生的 Rust `target/` 已删除；源码、`node_modules/`、`data/`、`data-next/`、根目录正式 EXE 和原始 RAR 均未删除。
- 尚存风险/下一步：下一节点应把已选集群计划接入旧版四阶段远程流程：创建 DIRECT/CROSSDOCK 草稿 → 读取草稿目的仓 → 查询草稿时段 → 手工确认后创建真实供应单；任何写入都不得在页面打开时触发。

### 2026-08-27：竞品商品信息采集失败复查（候选采集器待验收）

- 目的：根据用户反馈和商品 `4198227898` 的失败截图，重新阅读本文档中竞品跟踪模块的全部更新历史，核对当前 Python/Playwright 源码、随程序运行的 sidecar 二进制以及真实浏览器行为，修复价格、主图和商品身份信息采集。
- 历史结论复核：本文档此前已经记录“普通 Chrome 以及用户手工输入链接的 Edge 可以访问，隔离自动化 Edge 可能返回 Ozon 的俄文‘Похоже, нет соединения’合成拦截页”；也记录了 Chrome 优先、Edge 回退、独立持久化 profile、最终 URL 商品号校验、右上角可见价格和 Ozon CDN 主图解析策略。销量不是公开可信字段时继续留空，不填 0、不使用 AI 猜测，也不绕过验证码或平台访问限制。
- 本轮发现的直接问题：项目根目录正式 `competitor-collector.exe` 的时间为 2026-08-26 18:36:59、大小 48,259,645 字节，早于 2026-08-27 修改后的 `competitor_collector.py`。因此应用实际调用的是旧 sidecar，源码中的后续修改并未进入正式程序；截图里的 Edge 启动行为及 `--no-sandbox` 警告不能代表当前 Python 源码的最终行为。
- 已完成源码调整：`competitor_collector.py` 的浏览器顺序改为 Google Chrome 优先、Microsoft Edge 回退；Chrome 与 Edge 分别使用独立 profile 子目录，避免失败会话或站点状态相互污染；采集结果的 `source` 会记录实际使用的浏览器通道。没有加入隐藏自动化特征、Cookie 导入、验证码处理或其他绕过参数。
- 候选构建：由于机器没有可直接使用的系统 Python/`uv`，已使用工作区隔离依赖重新打包候选 `.collector-build/dist/competitor-collector.exe`。候选文件大小 48,849,281 字节，SHA-256 为 `35F534A9F1DB9B877151399358221E8343BD60706A33397B43E7D4BD0F12F7C6`。
- 真实运行结果：已直接以商品链接 `https://www.ozon.ru/product/4198227898/`、期望商品 ID `4198227898` 启动候选采集器。进程没有快速返回成功 JSON，而是持续等待页面达到可解析状态，说明真实链路仍存在页面受阻或稳定等待未结束的问题，尚未取得价格、主图写入 SQLite 的验收证据。
- 截图检查限制：尝试通过 Windows 界面检查能力读取正在运行的 Chrome 窗口时，工具无法以足够置信度确认当前 URL，按安全规则终止了界面检查。本节点没有尝试绕过页面限制，也没有把不确定页面误报为采集成功。
- 正式文件状态：**候选采集器尚未覆盖根目录正式 `competitor-collector.exe`，正式启动器也尚未重新构建。** 这是有意保留的安全边界，避免把只完成打包、但未完成真实页面与 SQLite 验收的候选版本作为正式交付。
- 修改模块/文件：`competitor_collector.py`、本 Markdown；候选构建位于临时 `.collector-build/`，不属于最终交付节点。
- 尚存风险/下一步：先取得候选进程的最终 stdout/stderr 和退出状态；若 Chrome 返回可识别的受阻页，应把 `status/final_url/page_title/evidence/browser` 明确写入失败 JSON，并有限次回退 Edge，而不是无限等待。只有在单独运行返回正确商品 ID、价格和主图后，才能替换正式 sidecar；随后必须执行 ERP 单任务采集、查询 `competitor_products`/`competitor_snapshots`、重新构建 Tauri Release，并补记正式 EXE 哈希和验收截图。

### 2026-08-27：竞品采集纯 Rust 复现与受阻错误修复

- 目的：解决 GitHub 正式版继续调用旧 `competitor-collector.exe`、浏览器显示 `--no-sandbox` 警告、Ozon 返回俄文“Похоже, нет соединения”后界面只记录 `TargetClosedError` 的问题；同时回答“是否可不用 Python”——正式竞品主流程现已由 Rust 完整复现。
- 根因确认：历史 SQLite 中商品 `4198227898` 等失败记录确实保存为 `blocked: TargetClosedError: Target page, context or browser has been closed`；截图中的真实页面同时是 Ozon 生成的“似乎没有连接”受阻页。前者只是 Playwright 页面关闭/清理阶段覆盖了更有用的诊断，后者是 Ozon 对独立自动化会话施加的访问限制，不是商品 URL、价格正则或 SQLite 写入错误。仓库根目录 sidecar 曾早于 Python 源码，也是 GitHub 下载后实际逻辑与源码不一致的直接发布问题。
- Rust 采集逻辑：`refresh_competitor_for_run` 先用 `ureq` 读取公开页并校验最终重定向 URL 的 Артикул；直连缺少价格、主图或完整图库证据时进入 `headless_chrome` 可见浏览器。浏览器按 Chrome → Edge 回退，分别使用持久化 profile，保留正常沙箱并移除库默认的 `--enable-automation`、后台网络禁用参数，使用 `ru-RU` 页面语言；没有导入 Cookie、隐藏自动化特征或绕过验证码。
- 旧版能力复现：Rust 页面脚本复用了固定左侧缩略图轨道识别、中央预览图排除、Ozon CDN `wc2000` 高清转换、图库去重、页面水合和稳定轮询；同时校验最终商品号、读取可见卢布售价，并保持销量未公开时为 `NULL`。单图商品可用浏览器图库模式证明完整性，普通直连仅有 `og:image` 时不会被误判为完整采集。
- 错误与证据：Ozon “似乎没有连接”会立即切换另一浏览器；验证码/安全验证页保留窗口等待人工完成。Chrome/Edge 都受阻时记录可读的 `blocked` 原因，页面关闭也转为受控诊断，不再向界面泄漏 `TargetClosedError`。`competitor_observations.evidence` 写入标题、原始价格、完整图库、图库模式和采集来源；`competitor_products`/`competitor_snapshots` 只在校验成功后写入主图与价格。
- Python 状态：`collect_competitor_python_html` 和旧扩展采集实现被编译条件明确排除，正式 EXE 中不再包含 `competitor-collector` sidecar 路径；根目录 `competitor_collector.py`、打包脚本和 5 个测试仅保留为兼容/迁移参考，不参与正式竞品任务。
- 修改模块/文件：`desktop-next/src-tauri/src/lib.rs`、`desktop-next/src-tauri/tauri.conf.json`、`competitor_collector.py`、`scripts/build-competitor-collector.ps1`、`tests/test_competitor_collector.py`、竞品任务前端文件及本 Markdown。
- 测试和构建结果：Rust 全库测试 23/23 通过，`cargo check --locked` 无 dead-code 告警，Python 兼容测试 5/5 通过，TypeScript/Vite 正式构建通过，Tauri Release 构建通过。根目录 `ozon-analytics-next.exe` 已更新，大小 25,680,896 字节，SHA-256 为 `FE6B7D1F75CD96D3BE4407D446F088D3018D6B179BEBBFE69E8764F1C1E2DAE8`。
- 验收边界：当前机器对真实商品 `4198227898` 的 Chrome/Edge 独立会话均返回 Ozon 受阻页，直接 HTTP 也进入 `__rr` 重定向循环，所以本轮不能诚实声称已取得新价格/主图快照；修复保证的是正确采集链路、正确错误分类和不再依赖旧 Python 二进制。最终线上成功验收仍需 Ozon 允许该会话访问或用户完成可操作验证页后，再从 ERP 单任务启动并查询新生成的 `competitor_observations`、`competitor_products`、`competitor_snapshots`。

### 2026-08-27：飞书供应链与库存管理状态联动

- 目的：让当前活动店铺的库存管理能够读取“飞书协作”中统一维护的供应链发货表，即使飞书供应链配置保存在另一个店铺数据库中，也不再因当前店铺缺少 App Token/Table ID 而断链。
- 公式核实：发货跟踪表公式字段“货物状态”（Field ID `fldcysFTRb`）按 `国内到库`、`国外到库`、`国外约仓` 与当天日期依次产生 `未送仓`、`在途`、`到达海外仓`、`已送仓`。同步改为严格使用这四个公式结果，不再用日期优先和状态文字模糊匹配。
- 库存口径：`未送仓`计入在生产；`在途`计入海外运输在途；`到达海外仓`计入海外已到仓；`已送仓`的供应链四个数量桶全部为 0，防止与 Ozon 已入仓库存重复计算。国内库存桶保留兼容字段，但当前公式没有独立“国内仓”状态，因此不虚构数量。
- 共享配置：同步优先读取当前店铺配置；若不完整，则只读扫描 `data-next/shops.json` 所列店铺数据库，选择第一套完整的飞书供应链凭证。远程读取使用来源店铺配置，记录仍写入当前活动店铺自己的 `shipment_tracking`，不复制密钥、不混写店铺业务库。
- SKU 关联：优先使用发货表显式 SKU；其次使用本地人工映射、飞书产品表非空的精确“品名→SKU”映射，以及当前店铺 products 中 SKU/货号/精确商品名。候选指向多个 SKU 时判定为歧义并留空，不做模糊猜测或数量分摊。
- 数据库变化：`shipment_tracking` 新增国外约仓日期、配置来源店铺和公式版本；新增 `feishu_supply_chain_product_mappings` 与 `feishu_supply_chain_sync_runs`，后者记录每次同步的总记录数、已匹配/未匹配数和四种状态数量，便于后续库存页面展示诊断。
- 修改模块/文件：`desktop-next/src-tauri/src/lib.rs`、本 Markdown。
- 测试和构建结果：`cargo check --locked` 通过；飞书供应链状态映射与 SKU 歧义保护测试 2/2 通过。
- 尚存风险/下一步：飞书产品基础信息表当前样例中的 SKU 多为空，未关联品名仍需在产品表补齐 SKU 或通过后续库存界面维护人工映射。代码未改写飞书表格；真实同步需要应用所在机器可访问飞书 API，完成后应核对 `feishu_supply_chain_sync_runs` 的未匹配统计。

### 2026-08-27：飞书发货批次一对多 SKU 补货明细

- 目的：修正“一条采购/跟踪批次只对应一个 SKU”的错误假设。类似批次 `CZ7046-OZON-0706-93` 可同时包含 `GJYB001-GREEN`、`GJYB001-BLUE`、`GJYB001-RED` 等多个 SKU，并分别记录补货数量。
- 交互：飞书协作的发货跟踪表新增“SKU 明细”列；点击“配置 SKU”弹出窗口，可按 SKU、货号或商品名搜索当前店铺产品，添加/删除多条 SKU 并填写每条补货数量。窗口实时展示批次总数、已分配和未分配数量。
- 校验：SKU 必须存在于当前店铺产品资料，数量必须为正整数；同一 SKU 重复填写时后端合并；明细总数不得超过飞书批次总数量。允许保留未分配数量，便于批次资料尚未录完时分阶段维护。
- 数据口径：新增 `shipment_sku_allocations(tracking_id, sku, quantity)`。库存管理的飞书供应链数量改为将该明细与 `shipment_tracking.cargo_status` 关联后汇总：未送仓→在生产、在途→海外在途、到达海外仓→海外已到、已送仓→不计入供应链库存。因此同一批次中的每个 SKU 都能获得自己的准确供应链数量。
- 飞书边界：SKU 拆分明细当前保存在各店铺本地数据库，没有改写飞书多维表格；再次从飞书同步批次状态和日期时不会删除已配置的 SKU 明细。
- 修改模块/文件：`desktop-next/src-tauri/src/lib.rs`、`desktop-next/src/types.ts`、`desktop-next/src/bridge.ts`、`desktop-next/src/OperationsPages.tsx`、`desktop-next/src/phase2.css`、本 Markdown。
- 测试和构建结果：`cargo check --locked` 通过；TypeScript 与 Vite 正式构建通过。

### 2026-08-27：固定正式构建与覆盖交付准则

- 问题：一次修改后使用了单独的 `cargo build --release` 并直接覆盖根目录 EXE，该构建没有通过 Tauri CLI 的正式构建上下文重新嵌入前端，启动后 WebView 仍访问 `localhost:1420`，因此显示 `ERR_CONNECTION_REFUSED`。
- 修复：关闭错误进程，改用唯一正式入口 `desktop-next/scripts/build-tauri-release.cmd`。该入口通过 Tauri CLI 执行 `beforeBuildCommand`，重新生成 Vite `dist` 并将正式资源嵌入 EXE。
- 开发准则：新增根目录 `DEVELOPMENT_RULES.md` 和 `AGENTS.md`，明确任何代码修改完成后都必须测试、使用 Tauri 正式构建入口、覆盖根目录 EXE、校验哈希、启动并做可见验收；禁止把单独的 Cargo Release 当作最终程序交付。

### 2026-08-27：飞书 TableIdNotFound 诊断与 SKU 列对齐

- 原因：飞书错误 `1254041 TableIdNotFound` 表示发货跟踪 Table ID 不存在，或该 Table ID 不属于当前填写的 App Token；常见误填是复制了 `view=` 后的视图 ID，或者组合了不同多维表格的 App Token 与 Table ID。
- 界面修复：SKU 配置按钮原先被插入到第二个单元格，导致后续“品名/店铺/状态/日期/通知”整行错位；已移动到表格最后的“SKU 明细”列，与表头一一对应。
- 错误提示：同步遇到 `TableIdNotFound` 时，界面现在明确提示从同一飞书 `/base/` 链接提取 App Token 和 `table=tbl...`，并显示实际使用的配置来源店铺，便于排查共享配置。

### 2026-08-27：FBO/FBS 批次差额审核与完结

- 业务规则：飞书批次进入“已申请/已送仓”后不再直接从供应链库存归零。未审核批次继续按各 SKU 的批次数量计入“海外到仓”，避免交货状态更新后数量无依据地消失。
- 审核交互：飞书协作的 SKU 明细列对待结算批次显示“审核差额并完结”。弹窗按 SKU 展示批次数量和 Ozon 当前已申请汇总，并要求填写本批次实际进入 FBO、转入 FBS、海外仓留存、短少/损耗、其他及备注。
- 平衡约束：每个 SKU 的五类处置数量必须全部非负且合计严格等于该 SKU 在本批次的数量；必须审核全部 SKU 才能确认完结。Ozon 已申请数量是 SKU 当前汇总，只作为人工核对参考，不自动猜测它属于哪个历史批次。
- 防重复：结算结果写入 `shipment_sku_allocations` 的处置字段并标记 `settled=1`。库存汇总仅对未结算的“已申请/已送仓”批次继续计算海外到仓；确认完结后该批次永久退出供应链库存计算，后续新批次不会与它重复。
- 修改模块/文件：Rust 数据库与命令、飞书协作结算弹窗、桥接类型、样式及本 Markdown。

### 2026-08-27：可安装的纯本地正式发布包

- 原安装版问题：开发目录旁已有 `data-next`，但 NSIS 安装到新电脑后只有程序文件，没有可用于首次启动的数据库；同时系统安装目录通常不适合直接写业务数据库，因此可能出现 EXE 存在但启动失败。
- 本地数据架构：正式安装版内嵌不含用户数据和密钥的空白 SQLite 模板。首次启动自动复制到 `%LOCALAPPDATA%\com.ozonanalytics.desktop\data-next`；之后店铺、库存、成本、飞书映射、批次结算和设置均保存在本机。Ozon/飞书/WB 功能仅在用户主动同步时访问平台官方 API，不依赖自建云端服务器。
- 隐私：空白模板经检查 `settings=0`、`products=0`，不包含开发数据库、店铺数据、API 凭证或业务记录；敏感配置继续由 Windows DPAPI 加密。
- 发布物：NSIS 安装包、带空白资源模板的便携 ZIP、排除 `data/`、`data-next/`、构建缓存、用户数据库和密钥的源码 ZIP。
- 验收：在项目目录之外启动便携版，进程正常响应并成功创建本地 `data-next/shops.json` 和 `ozon_next_default.db`；本地初始化回归测试加入 Rust 测试集，全库测试 26/26 通过，前端正式构建通过。

### 2026-08-27：竞品采集收敛为主图与看板图片适配

- 采集口径：竞品跟踪完成条件改为商品身份校验通过且取得主图；不再要求完整图库，也不会仅因缺少第二张图片或图库轨道证据进入专用浏览器。售价和公开销量存在时仍可作为辅助快照保存，但不再阻塞主图任务完成。
- 浏览器回退：直连页面只有在缺少可验证主图时才进入专用 Chrome/Edge；减少无必要的页面加载、验证等待和采集失败面。
- 看板显示：竞品卡片主图区域从 88px 提升到 180px，图片统一填满卡片并居中裁切，避免不同宽高比导致卡片高度跳动或主图过小。
- 修改模块/文件：`desktop-next/src-tauri/src/lib.rs`、`desktop-next/src/OperationsPages.tsx`、`desktop-next/src/phase2.css`、本 Markdown。

### 2026-08-28：系统浏览器 CDP 连接时序修复与真实商品验收

- 断点根因：系统 Chrome/Edge 已使用空闲远程调试端口成功启动，但 Rust 在 CDP 刚连接后立即读取标签页；此时目标商品页尚未注册，因启动时序竞争错误报告“没有创建商品标签页”。
- 修复：CDP 连接后有限等待目标 `ozon.ru/product/` 标签页；仍未出现时，通过已连接的真实系统浏览器会话创建新标签页并导航到目标 URL。继续保留独立 profile、软件渲染、最终 Артикул 校验和受阻页识别，不加入验证码绕过或隐藏自动化特征。
- 真实验收：可见系统浏览器对商品 `2846376063` 的 Rust 回归测试通过，完成页面打开、CDP 连接、商品身份校验、HTML 与主图证据返回。旧 Python/Playwright 候选对同一商品的 Chrome/Edge 均收到 Ozon“Похоже, нет соединения”受阻页，证明旧链路不能作为成功回退。
- 测试强化：真实浏览器测试除商品号/标题外，新增 `codexMainImage` 或 `og:image` 主图证据断言，避免仅打开窗口即误报采集成功。

### 2026-08-27：月度盈亏已核算销量口径修复与重新安装

- 问题定位：2026 年 7 月月度盈亏显示“已核算销量 1112 件”，原因不是 Finance 只有一千余件，而是 `delivery_events` 仅覆盖 7 月 26–31 日的部分迁移历史。旧查询对每个 SKU 优先使用 Posting 妥投数，只要该 SKU 存在一条 Posting 记录，就会丢弃该 SKU 更完整的 Finance 已交付记录，最终得到 `840 + 272 = 1112` 的混合欠计结果。
- 修复口径：月度 Finance 盈亏以 `finance_transactions` 中 `OperationAgentDeliveredToCustomer` 的去重 operation ID 作为每个 SKU 的已交付数量；仅当该 SKU 完全没有可归属的 Finance 已交付记录时，才回退 `delivery_events`。缺成本 SKU 检查使用同一口径，避免报表和缺成本窗口再次不一致。
- 当前数据复核：本土店 2026 年 7 月 Finance 已交付为 5902 件；修复后已核算销量应由 1112 件恢复为 5902 件。按当前成本与汇率缓存，采购成本约为 ₽1,757,698.60、头程约为 ₽559,164.00，税前利润估算约为 ₽1,029,043.25；最终页面结果仍以当前数据库重新计算为准。
- 缓存：月度报表指纹升级为 `finance-v6-finance-delivered-first`，旧的错误报表缓存会自动失效，无需删除业务数据。
- 界面说明：已核算销量详情明确标注“Finance 已交付优先；无记录时回退 Posting 妥投”。
- 测试：`cargo check --locked` 通过；新增 Finance 优先/Posting 回退 SQL 回归测试 1/1 通过；`pnpm build` 通过。`cargo fmt --check` 仍报告 `lib.rs` 中既有的大量全文件格式差异，本次未批量格式化，以免改写无关用户源码。
- 构建与安装：已使用强制入口 `desktop-next/scripts/build-tauri-release.cmd` 构建正式 EXE，并额外生成 NSIS 安装包。根目录 EXE 与正式 Release 产物 SHA-256 均为 `864FE1C44181BDC7047373C7BEA7AC59B91577AD5862017F07E65FEB89044E4A`；安装包 SHA-256 为 `424564A831B6E4FB69DEB51C9D40B0E87817A3A4F077FDE6981DEE27D7B381B1`。NSIS 已静默安装成功，开始菜单入口指向 `D:\Users\wufeifan\AppData\Local\Ozon Analytics\ozon-analytics-next.exe`。
- 验收：已安装程序成功启动并出现标题为“Оzon ERP”的本地窗口，说明没有停留在 `localhost` 拒绝连接状态。进一步的可见页面截图检查由用户按 Escape 停止，因此本次记录不声称完成月度盈亏页面的最终人工截图验收。

### 2026-08-28：经营总览单品价格读取与安全改价

- 功能位置：经营总览 → 单品与产品系列经营 → 双击单品。详情窗口新增“Seller API 商品价格”区域，展示当前售价、划线价、最低价、结算净价、币种与最近同步时间。
- 读取逻辑：使用 Seller API `POST /v5/product/info/prices`，优先按货号查询、缺少货号时按 Ozon product ID 查询；结果写入独立的 `product_price_cache`。已有缓存时立即打开详情，首次无缓存时自动读取，同时保留“读取最新价格”按钮，避免每次打开弹窗都阻塞等待网络。
- 改价逻辑：使用 Seller API `POST /v1/product/import/prices`。提交前校验售价大于零、划线价不得低于售价、最低价不得高于售价；用户还必须输入“确认改价”并通过系统二次确认。程序提交后重新读取价格，区分 `verified` 与 `accepted`，不会把尚未回读的数据误报为已确认。
- 审计记录：新增 `product_price_action_logs`，保存 SKU、货号、改价前售价、请求售价/划线价/最低价、Ozon 原始响应、状态、提示、回读售价和时间；最近记录显示在单品详情中。价格缓存和改价日志与商品趋势表分离，不修改销量、订单或成本口径。
- 修改文件：`desktop-next/src-tauri/src/insights.rs`、`desktop-next/src-tauri/src/lib.rs`、`desktop-next/src/ProductInsights.tsx`、`desktop-next/src/bridge.ts`、`desktop-next/src/types.ts`、`desktop-next/src/product-insights.css`。
- 测试：`cargo check --locked` 通过；价格关系回归测试 2/2 通过；`pnpm build` 通过。为避免真实店铺价格被测试修改，本次没有执行写价格操作。
- 正式构建：已使用唯一正式入口 `desktop-next/scripts/build-tauri-release.cmd` 完成 Release 构建并覆盖根目录 `ozon-analytics-next.exe`；Release 与根目录文件均为 25,770,496 字节，SHA-256 均为 `5A21F1FB7CE515D1E4097E10C246C3B0577C3D85CB3B4C60A1DEFBC96864FBBD`。
- 可见验收：根目录正式程序已实际启动，窗口标题为“Ozon ERP”，经营总览及“单品与产品系列经营”区域正常显示，确认使用内嵌前端而非 `localhost:1420`。当前打开的默认本土店没有本地商品记录，无法在不新增/篡改业务数据的前提下双击真实单品验证价格卡片；价格读取仍需在有商品且已配置 Seller API 的店铺进行最终数据验收。

### 2026-08-28：启用 Archify 项目架构图工作流

- 已使用项目真实入口、Tauri IPC、Rust 命令注册、SQLite 数据域、Ozon/WB/飞书集成及竞品采集源码作为证据，新增 `docs/architecture/ozon-erp-runtime.architecture.json`。
- 图中主链路为“运营人员 → React 工作台 → Tauri IPC → Rust 业务核心 → SQLite”，并独立呈现 Ozon Seller API、Wildberries API、飞书开放平台和竞品公开页采集分支；没有把文件邻近关系推断成运行调用关系。
- Archify Schema 校验已通过。Showcase 组合检查最初报告 3 个标签/线路间距问题，两轮定向修正后剩余 1 个“事务与快照”标签与 SQLite 节点重叠诊断。依照 Archify 最多两轮视觉几何修正规则，本次停止继续猜测坐标，不生成或冒充已通过的最终 HTML；JSON 保留为下一轮可继续处理的候选规范。
- 后续验证节点：已按 Archify 建议用 `labelAt [1104,422]` 消除最后的标签冲突，线路、标签和箭头检查一度达到 9/9；随后 Showcase 在 1440×900 检出 5.65px 的投影文字低于 6px 下限。收窄画布后又准确暴露 SQLite 与飞书节点不足 8px 的间距约束。由于本轮已达到两次定向修正上限，当前仍未交付 HTML，下一轮应只处理飞书节点位置与画布宽度的组合，不改动已经通过的连接布局。
- 最终交付：飞书节点上移后，`docs/architecture/ozon-erp-runtime.architecture.json` 通过 Archify Showcase 9/9 检查，组合结果为 0 错误、0 警告；已原子生成 `docs/architecture/ozon-erp-runtime.html`。规范 SHA-256 为 `DE3FC86D34ACDE41213090275AF37206358F683E05926160183D3F7B8A8FB03E`，HTML SHA-256 为 `36BDEE2C25208607B545FFD6233C5AE0BA5BA96A05E024C35862FD39924B2662`。
- 视觉验收：Archify `visual-check` 在 1440×900、1600×1000、1920×1080、2048×1320 全部通过，无横向或纵向溢出，最小投影文字 6.62px；人工查看 1440×900 浅色与 2048×1320 深色截图，主链路、平台分支、采集链路、图例与三张结论卡片均清晰，无节点遮挡、线路穿框或明显空白失衡。`visual_review: passed`，本轮修正次数 1。

### 2026-08-28：上品模块实时属性与提交前校验迁移

- 迁移依据：重新核对本文档 RFBS 上品章节、旧版 `legacy-sources/rfbs-listing-tool/core.py` 的 `reference_fact_map`、`set_attribute_value`、`validate_required`，以及 `services.py` 的类目属性/字典接口；同时对照竞品跟踪模块的显式验证页识别、商品证据优先和失败原因分类。
- 商品采集：上品参考页解析新增显式 Antibot/Captcha 页面识别，不再把验证页标题或占位图当作商品；JSON-LD 不完整时增加 Ozon 页面内嵌商品名回退；商品图片保持页面原始顺序去重，不再排序后破坏主图顺序。浏览器采集继续使用独立 `listing_browser_profile`，不会在软件启动时自动采集。
- 属性定义：新增 Seller API `/v1/description-category/attribute` 实时读取，前端在选定类目/type 后展示属性 ID、必填状态、字典/自由文本类型和组合属性分组。新增 `/v1/description-category/attribute/values/search` 桥接，为下一步结构化字典选择控件提供真实选项，禁止猜测字典 ID。
- 保守映射：参考页参数仅在属性名称标准化后完全一致、且目标是自由文本属性时自动写入；字典属性一律留给人工从 Ozon 实时字典选择。每个自动值保存 `_source=reference_exact`，任务 payload 同时记录映射模式、数量和边界说明，便于审计。
- 提交前校验：新增货号、俄文标题、售价、HTTPS 图片、重量/尺寸及 Ozon 实时必填属性校验；界面明确列出缺失项和缺少的必填属性名称。校验只读，不调用 `/v3/product/import`，因此本轮不会误创建或修改真实 Ozon 商品。
- 修改文件：`desktop-next/src-tauri/src/listing.rs`、`desktop-next/src-tauri/src/lib.rs`、`desktop-next/src/types.ts`、`desktop-next/src/bridge.ts`、`desktop-next/src/OperationsPages.tsx`、本 Markdown。
- 测试：`cargo check --locked` 通过；上品模块测试 7/7 通过（含新增显式验证页拒绝测试）；TypeScript 与 Vite 正式构建通过。
- 正式构建与覆盖：已使用唯一入口 `desktop-next/scripts/build-tauri-release.cmd` 完成 Tauri Release 构建并覆盖根目录 `ozon-analytics-next.exe`。Release 与根目录 EXE 均为 26,178,560 字节，SHA-256 均为 `CDCED788BDF84B63C410AB4CAA42A42118AF31338B448F788F5D9BFA51C0AF48`。
- 启动验收：根目录新进程 PID 36400 保持运行且 Windows 报告可响应，但没有生成可枚举的主窗口句柄；关闭旧进程时系统同时返回过一次“Access denied”，推测当前桌面会话还存在其他权限上下文中的旧实例或残留锁。本次不声称完成上品页面的最终可见验收；源码、正式构建与覆盖哈希已经验收，需在用户关闭所有旧实例后再次启动并进入“跨境运营 → 跨境上品”检查实时属性卡片。
- 剩余工作：字典属性的前端搜索/选择编辑器、组合属性多组编辑器、正式 `/v3/product/import`、导入任务轮询/错误明细以及成功回写产品台账仍未完成。正式提交必须在这些字段编辑和二次确认闭环后再开放，不能直接把未校验 JSON 发往真实店铺。

### 2026-08-28：跨境上品结构化属性编辑器迁移

- 迁移目标：继续复用旧版 RFBS 上品工具的 `set_attribute_value`、字典人工选择、普通/组合属性维护和必填检查规则，结束主要依赖手写 `attributes` / `complex_attributes` JSON 的操作方式。
- Rust 写入边界：新增 `set_listing_attribute_value` 与 `clear_listing_attribute_value`。普通属性和组合属性统一由后端定位、创建或更新；非多值属性只保留一个当前值，多值属性按字典 ID 或文本去重；清除组合属性后自动删除空分组，避免生成 Ozon 不接受的空结构。
- 字典属性：页面接通 `/v1/description-category/attribute/values/search`，操作员输入关键字后加载 Ozon 官方字典，并通过下拉框明确选择。程序不根据中文名称或相似度猜测字典 ID，选择结果同时保留显示值和 `dictionary_value_id`。
- 自由文本：逐属性提供俄文输入和保存按钮。Rust 后端拒绝 seller-entered 中文自由文本，避免把仅用于界面/字典显示的中文错误提交给 Ozon；字典显示值允许由官方接口返回。
- 可见状态：属性表直接显示当前值、必填缺失、多值标记、普通/组合分组、描述以及清除入口。原始 JSON 编辑区降级为折叠的高级工具，便于诊断但不再是主要业务入口。
- 安全边界：本节点只编辑本地 `listing_jobs` 草稿并执行只读提交前校验，没有开放 `/v3/product/import`，不会在测试或页面操作中创建真实 Ozon 商品。下一阶段仍需完成导入 payload 元数据清洗、用户二次确认、单次写入、任务状态轮询、错误明细和产品台账回写。
- 回归测试：新增普通自由文本、官方字典值、组合属性、多值去重、清除空组合组和中文自由文本拒绝测试。`cargo check --locked` 通过；Rust 全库 33 项通过、1 项需要可见浏览器和真实 Ozon 的测试按设计忽略；`pnpm build` 与 Tauri `beforeBuildCommand` 前端构建通过。
- 正式发布：使用 `desktop-next/scripts/build-tauri-release.cmd` 生成内嵌正式前端的 Release EXE，并用 `build-tauri-package.cmd` 生成 NSIS 安装包；根目录 EXE已覆盖，便携包和源码包已重建。EXE SHA-256 `21BD046E08D6C18512A2486088A310C3ADB5FF5E094610432EF57490C211663C`；安装包 `CB924576488AD725C386E4BE3A781DB10DD0981F1DB6C362D413FD3C08974E66`；便携包 `9CC18D3AA424DF9021158EAB90A3197E7C4F5AEBE72F6B8B683F9227CA507770`；源码包 `378B9E657872897C8B88B12EEFDC91AD5AE33EFF3CF3721DD10434476B6DF94C`。
- 启动验收：根目录 `ozon-analytics-next.exe` 已启动，进程 PID 8852，Windows 报告可响应，窗口标题为 `Ozon ERP`，确认没有回退到 `localhost:1420`。由于真实类目字典加载依赖当前店铺 Seller API 配置，本次未替用户选择或写入真实商品属性；仍需在已配置店铺打开“跨境运营 → 跨境上品”，选择一个草稿类目后完成一次真实字典读取验收。

### 2026-08-28：跨境上品模式、自动货号、CNY 与 AI 必填属性迁移

- 迁移依据：对照旧版 `legacy-sources/rfbs-listing-tool/app.py` 的工作台模式切换、`AUTO-YYYYMMDD-XXXXXX` 货号初始化、自动流程第 5 阶段，以及 `services.py::CopywritingService.map_attributes` / `choose_dictionary_values` 的提示词和 ID 白名单纪律。附件截图只用于确认当前页面缺少这些入口，没有把截图文字当作执行指令。
- 上品模式：草稿创建区新增“跟卖模式”和“本地新品模式”。跟卖模式必须提供 Ozon 链接或 Артикул，并保留采集、浏览器验证与参考属性映射；本地新品模式不要求来源，不允许执行参考商品采集，使用人工标题、图片地址、类目和属性。任务 payload 持久化 `listing_mode`，后续保存不会丢失模式。
- 自动货号：新建两种模式草稿时均由 Rust 自动生成 `AUTO-YYYYMMDD-XXXXXX`，同时写入 `listing_jobs.offer_id` 和 payload；编辑页将 offer_id 设为只读，避免同一任务在后续阶段被误改成另一个商品身份。
- CNY 口径：根据当前跨境店铺新业务规则，草稿固定保存 `currency_code=CNY`，界面字段改为“跨境售价 CNY”。这项规则明确覆盖旧版 Ozon RFBS 工作台默认显示 RUB 的行为；ROI 核价中的采购、贴单、运费、利润继续按 CNY 计算。
- AI 必填属性：新增 `ai_fill_listing_required_attributes`。程序先保存当前草稿，再读取人工指定类目的实时属性，只把尚未填写的必填属性交给 AI；类目仍必须人工选择，AI 不改变类目。
- 提示词与证据纪律：AI 只能返回已提供的属性 ID；自由文本必须是俄文；品牌、型号、尺寸、重量、材质/成分、产地/制造商、认证、质保、包装数量/内容物、容量和兼容性在没有直接事实时不得推断。页面标题、描述、采集参数及人工重量尺寸是允许证据，AI 结果记录 `prompt_version=legacy-rfbs-v1` 和剩余必填项。
- 字典安全：有搜索词时通过 Ozon `/v1/description-category/attribute/values/search` 获取最多 100 个候选；空搜索和 AI 初始候选改用 `/v1/description-category/attribute/values`、`last_value_id=0` 的正式分页接口，单页最多 2000 项。AI 只能选择候选中已存在的 `dictionary_value_id`；返回不存在的 ID、属性外 ID或显示值不一致时不会采用，显示值最终以 Ozon 官方候选为准。没有等义选项的必填属性继续留空并提示人工处理。
- 写入边界：AI 只更新本地 `listing_jobs` 草稿，没有调用 `/v3/product/import`；没有配置 AI Base URL、模型和 API Key 时明确引导到连接设置。真实 AI 请求和真实字典读取未在自动测试中执行，避免消耗用户额度或依赖在线店铺配置。
- 测试：`cargo check --locked` 通过；Rust 全库 35 项通过、1 项真实浏览器测试按设计忽略；新增自动货号格式和 AI 严格 JSON 代码围栏解析测试；`pnpm build`、Tauri `beforeBuildCommand` 和 NSIS 构建通过。
- 可见验收修复：首次正式构建后使用 Windows 可访问性树进入“跨境上品”，确认模式单选、CNY 售价和 AI 按钮均已显示；同时发现空搜索调用 `/values/search` 返回 HTTP 400，因此没有把该候选构建交付。已按旧版 `dictionary_values_all` 改用 `/values` 分页接口，并再次完成全套测试、构建和打包。
- 正式发布：最终正式 EXE 与根目录覆盖文件 SHA-256 均为 `D02A43A83DA8C9C65AB41F6F7599C3B3FB21056B70A50EED7C0AB54C9DAE63F4`；安装包 `ED82D3CBD98C6E54B5AE77AF4117484D16125CC6D37F90E7538A84A596F47585`；便携包 `69D502B805E7398E4E4284F4A8295945870530B2B695C7BD7A3B0AF9492081FA`；源码包 `A81B8B01CD6CC92A262A8744583096FEDEA7CF0BC1EF7CAE29FFF6357CB88D25`。
- 启动验收：最终根目录正式程序 PID 39132 正常响应，窗口标题 `Ozon ERP`，确认加载正式内嵌前端而不是 localhost。真实 AI 请求仍未自动触发；下一次业务验收应分别创建一个跟卖草稿和一个本地新品草稿，再在已配置 Seller API/AI 的店铺读取类目并点击“AI 填写必填属性”，核对未填写项确实是缺乏证据或没有官方等义字典值。

### 2026-08-28：砍掉跨境上品并改为产品台账、跨境订单口径修复

- 模块收缩：导航“跨境上品”改为“产品台账”，用户可见页面只保留 Excel 台账路径、店铺筛选、产品资料、采购成本、重量尺寸和 1688 采购链接；采集、类目、AI 属性、核价和自动上架页面不再从该入口暴露。旧代码暂时保留为未路由实现，避免在当前存在大量并行迁移改动时进行高风险删除。
- 台账缓存：Rust 端按“文件路径 + 修改时间”缓存解析后的工作表；React 端缓存台账设置和查询结果，切换页面不会重复读取。保存配置、手动“重新读取台账”或成本同步后会主动失效缓存。平台列为空的旧 Ozon 台账行继续可读。
- 成本写入边界：同步逻辑只更新当前店铺 SQLite 中已经存在且货号完全相同的 SKU，不创建产品。用户提供台账共有 20 条记录，主要店铺为 `xingyan2`、`非凡智汇`，货号主要为 `AUTO-202608...`；当前 KJYD/跨境订单货号主要为 `XJ-*`、`WZW*`、`GJYB*`，没有可安全确认的同货号记录，因此本轮数据库成本保持不变，也没有自动生成映射。
- 1688 跳转：订单返回值增加台账供应商链接；只有当前订单货号命中台账且链接域名为 `1688.com` 或其子域时才显示“打开 1688”。Rust 命令在真正打开系统浏览器前再次校验域名，台账中误填的 Ozon 或其他网址不会被当作供应商链接。
- 订单履约拆分：跨境店订单查询只保留 `RFBS`、`FBP`、`WHD`，本土店只保留 `FBO`、`FBS`；跨境经营页中的“FBS / 卖家仓”纠正为“RFBS / 卖家仓”。订单页标题和说明会随店铺类型显示“跨境订单中心”或“本土订单中心”。
- 金额修复：`posting_routes.order_price` 在跨境订单中已经是 CNY，订单中心不再用人民币金额除以 `cross_border_rub_per_cny`；基础配送估算也保持当前订单数据口径，不在订单展示层二次换汇。其他利润报表的既有 RUB/CNY 规则未被扩大修改。
- 订单缓存：订单查询按当前店铺、日期范围和搜索词缓存；普通切页复用缓存，切换店铺或点击刷新时清除缓存，避免重复读取同一批订单。
- Archify 分析：新增 `docs/architecture/cross-border-order-ledger.dataflow.json` 与交互式 `cross-border-order-ledger.html`，覆盖 Ozon API、订单同步、店铺 SQLite、履约过滤、Excel 台账缓存、成本精确匹配、1688 域名校验和前端模块。Showcase 校验 9/9、0 错误、0 警告，规范 SHA-256 `64634A0A0A535BC184DC1F4D743153BB4886DF3D5C92FE59D8DF673D57776BCD`，HTML SHA-256 `07C8AAB15E5261E3B39E6008FBE41D1B549964E60B58863C9854ABDDCD3A36AF`。人工查看浅色截图后节点、标签与线路清晰；`visual-check` 的可读性和截图捕获通过，但严格单屏 containment 因纵向滚动失败，因此不标记为完整视觉通过。
- 测试：`cargo check --locked` 通过；`cargo test --locked --lib` 为 35 项通过、0 失败、1 项真实浏览器测试按设计忽略；`pnpm build` 通过；正式 Release 与 NSIS 构建均通过。
- 正式交付：使用 `desktop-next/scripts/build-tauri-release.cmd` 生成内嵌正式前端的 Release，并覆盖根目录 `ozon-analytics-next.exe`；Release 与根目录文件均为 26,316,800 字节，SHA-256 `393536798661E95954FBD9A132258A45E164A8028D883C5B3CBD9C6368CE0ADA`。安装包 SHA-256 `97CE6EDD61F471FF3FC51A20F3092DA399487B535159D491B2835758B73B4084`；便携包 `01F2FA4B7D5D8F57E3CF11AF171A0E7B871E6E908A01B3B539B9CF12A95C2F46`；源码包 `624282DA8E2DBE0256E9760CE7F4C05F90C93C42A80B6C73509C0F266A574CD0`。
- 可见验收：根目录正式程序已启动并返回唯一窗口 `Ozon ERP`，店铺管理页面和当前跨境店正常显示，确认加载的是内嵌正式前端而不是 `localhost:1420`。由于验收期间用户在同一桌面切换了前台窗口，自动化未继续点击订单和台账入口；页面级最终验收仍建议由用户在当前正式程序中进入“订单中心”和“跨境运营 → 产品台账”核对真实数据。

### 2026-08-28：经营总览新增可选日期的全量数据同步

- 入口位置：经营总览页头右侧、原“刷新数据”按钮之前新增开始日期、结束日期和“同步所有数据”按钮，对应用户截图红框位置。原“刷新数据”仍只负责从本地数据库重新读取看板，两种操作语义保持分离。
- 功能复用：总览直接调用数据同步中心同一个 `sync_all_data` Tauri 命令，Seller 销量、Performance 广告和 Finance 结算继续由三个后台线程并行执行，没有复制另一套后端同步口径。
- 日期规则：默认使用当前看板日期范围；允许操作员分别选择开始和结束日期。开始日期不得晚于结束日期，结束日期不得晚于本机今天，非法范围会禁用同步按钮并显示明确提示。
- 结果反馈：同步期间显示旋转状态和“同步中”；完成后分别展示 Seller、Performance、Finance 的写入行数或具体错误。即使某个来源失败，也会保留另外两个来源的独立结果。同步完成后清除报表缓存并自动重新读取经营总览。
- 修改文件：`desktop-next/src/App.tsx`、`desktop-next/src/phase2.css`、本 Markdown。截图只用于确认入口位置，没有把截图内容作为程序指令。
- 测试：`cargo fmt --check`、`cargo check --locked`、`pnpm build` 均通过；Rust 全库 35 项通过、0 失败、1 项真实浏览器测试按设计忽略。
- 正式发布：已使用 `desktop-next/scripts/build-tauri-release.cmd` 和 `build-tauri-package.cmd` 重建正式程序、NSIS 安装包、便携包和源码包。Release 与根目录 EXE 均为 26,316,800 字节，SHA-256 `AEE86CF6929EA6630EE1291D7C88C9999AB5D908453D5F8F0E168C8C4B1A38D5`；安装包 `81BEBF34413B5DC0E7E84684E3E6FFB401C7A58CDA3BA9636B517F32545C2E6A`；便携包 `374A6FDB76056CAAC4B102C7FB6D029E6FC5740188B11250404BA9C0D6CF5177`；源码包 `BAD64E35C6FCAF39E21C8E4A4826F806E01600CB3CF888227066ECA1FB6228E5`。
- 启动验收：根目录正式程序成功启动并返回唯一窗口 `Ozon ERP`。准备读取经营总览截图时，用户通过物理 Escape 键停止了 Windows 自动化，因此本轮按要求立即停止后续界面控制，不声称完成按钮的最终可见点击验收；源码、测试、正式构建、覆盖哈希和启动窗口均已验证。

### 2026-08-28：店铺不可变专属 ID 与重命名数据保护

- 原因核查：店铺注册表从设计上已经使用 `RawShop.id` 作为身份，并通过 `database_file` 指向独立 SQLite；`update_shop` 原逻辑只更新名称、类型和 API 配置名，不会根据名称创建或切换数据库。因此把 KJYD 重命名为“非凡智汇”不会删除原库。截图只用于确认用户看到的店铺管理状态。
- 数据证据：原 KJYD 专属 ID 为 `c115c8fc976d`，数据库仍为 `data-next/shops/shop_next_c115c8fc976d.db`，大小约 10.9 MB。只读核查确认其中仍保存 `sales_daily` 6,959 行、`posting_routes` 717 行、`products` 710 行、`ad_daily` 600 行、`finance_transactions` 8,859 行，业务数据没有消失。
- 身份固化：店铺列表返回数据库文件大小；店铺卡片直接显示“专属 ID”和本地数据库大小。编辑窗口同样显示不可修改 ID，并明确提示重命名不会更换数据库。保存成功提示会回显该 ID。
- 后端保护：重命名开始前保存原 ID 与数据库路径，写注册表前再次检查两者没有变化；若未来代码误改身份或数据库绑定，操作会被拒绝。新增店铺仍自动生成独立 ID，名称仅作为可变显示字段。
- 修改文件：`desktop-next/src-tauri/src/lib.rs`、`desktop-next/src/types.ts`、`desktop-next/src/bridge.ts`、`desktop-next/src/Phase2Pages.tsx`、`desktop-next/src/phase2.css`、本 Markdown。
- 测试：`cargo fmt --check`、`cargo check --locked`、`pnpm build` 通过；Rust 全库 35 项通过、0 失败、1 项真实浏览器测试按设计忽略。
- 正式交付：已使用唯一正式入口重建 Release 与 NSIS，并重建便携包和源码包。根目录与 Release EXE 均为 26,317,824 字节，SHA-256 `F77E6B0D5AB6DF6024AEB9703E41B700142AB319B9D8D496FAA678C183BCA192`；安装包 `AD04A457347B376320C5C7636497992E128D7CD19CBD3519FB3AE2CA1F786813`；便携包 `717B11C0DE640B49DE0901B85D6C4FA0112308E159B3427664E4CB69C90C71AC`；源码包 `FDF7BC0B2501486BB50B512DA9BF316F76BA1220888E7A123B6BA207B891A681`。
- 历史履约兼容：可见验收时跨境订单中心显示 0 笔，进一步只读核查发现 717 条历史路由仍在，但旧同步以 `FBS` 保存 692 条、以 `FBO` 保存 25 条。跨境筛选现兼容旧值，并在界面映射为 `FBS → RFBS`、`FBO → FBP`；不会修改原始历史记录。本土店仍按 FBO/FBS 展示。兼容修复后的最终根目录 EXE 为 26,319,360 字节，SHA-256 `D4A54302B3B910E981EA4064C312C455C866DF275EEA55263F3EA15F19E71C2A`。

### 2026-08-28：智能增量同步与强制覆盖同步

- 同步模式：经营总览和数据同步中心均新增“智能增量 / 强制覆盖”选择。默认智能增量；强制覆盖会绕过本地覆盖范围、Seller 历史跳过和断点增量起点，完整重新请求所选日期。
- Seller：继续使用 `sync_progress` 分页断点，同一范围失败后从 offset 继续；完整稳定历史范围直接复用本地缓存。智能同步已有范围时从已缓存最大日期回刷，强制模式从用户选择的开始日期重新获取。
- Performance：智能模式完整历史范围已覆盖时不调用远端 API；范围包含近期或新增日期时，只从本地最大日期与最近 3 天窗口的较早者开始，保证新数据补齐并回刷最近三天的广告归因。强制模式完整获取所选范围。
- Finance：智能模式完整且稳定的历史范围直接复用本地缓存；默认回刷最近 45 天，以覆盖当前月、上月及平台追溯结算调整。强制模式仍按月分页完整获取所选范围，并在成功请求后事务性替换范围内旧 Finance 数据。
- 数据安全：Seller、Performance 继续使用唯一键 UPSERT，Finance 继续使用“请求全部成功后才删除并替换”的事务逻辑；智能同步不会因为跳过 API 而删除任何本地数据。同步结果为 0 行时，前端明确显示“复用本地缓存”。
- 测试：新增“完整历史范围命中缓存”和“未完整范围从最大缓存日期继续”两项回归测试；Rust 全库 37 项通过、0 失败、1 项真实浏览器测试按设计忽略；`cargo check --locked` 与 `pnpm build` 通过。
- 正式交付：使用正式 Release 和 NSIS 入口重建并覆盖根目录程序，同时重建便携包和源码包。EXE SHA-256 `57642C168EFE4C9BC20A87EC60DC6BF688A7B599C324A5642C31362A6312D533`；安装包 `1A323E5D3024D287DBB8507E45A8BA81EE4B5AB8C295593DEEAB3C95547F66DD`；便携包 `A03DA66346D9D4237369CAB9A2F0CF2C1D172ACC8D4C4486E92427F96E47DA20`；源码包 `A5C0DEB93E19A4C3A72099D12EF915A4DAE1E7E5BE06C5DBE4B9F5CCE9BF4897`。

### 2026-08-28：WB API 功能完善前准备节点

- 节点性质：用户下一阶段将集中完善 Wildberries API。本节点仅完成现状盘点、范围约束和后续验收基线，不代表新增接口已经实现，也没有调用真实 WB API、改写 WB 业务数据或重新发布程序。
- 当前工作区：React 已提供独立 `WB ERP` 工作区及经营、成本、API 设置页面；Tauri 后端集中在 `desktop-next/src-tauri/src/wb.rs`，前端入口主要位于 `desktop-next/src/OperationsPages.tsx`，IPC 封装位于 `desktop-next/src/bridge.ts`，命令在 `desktop-next/src-tauri/src/lib.rs` 注册。
- 当前数据隔离：WB 使用 `data-next/wb/wb_analytics.db` 独立 SQLite，不与 Ozon 店铺数据库混用；历史迁移只把旧 `wb/wb_analytics.db` 快照复制到新版 WB 数据域。后续扩展必须继续保持 WB Token、缓存、同步进度和报表数据与 Ozon 店铺数据隔离。
- 已存在的只读能力：当前代码已经覆盖订单、广告活动及商品级广告统计、WB 仓库目录、卖家仓目录和仓库库存报表；主要远端域包括 `statistics-api.wildberries.ru`、`advert-api.wildberries.ru`、`marketplace-api.wildberries.ru` 和 `seller-analytics-api.wildberries.ru`。这些只是现有源码实现清单，下一阶段仍需逐项依据届时 WB 官方文档核对 URL、版本、字段、权限、分页、限流和停用状态。
- 已存在的本地能力：产品成本维护、每日经营快照、WB 专用飞书周报、Token 配置导入/导出已经接入。Token 本机保存使用现有加密机制；导出的配置包含明文 Token，界面已有风险提示。后续不得把 Token、请求头或完整响应中的敏感字段写入日志和 Git。
- 优先审计项：先建立“功能 → 官方接口 → Token 权限 → 请求参数 → 分页/限流 → 本地表 → 页面指标”的映射表，再处理广告活动为 0、库存口径、订单状态/退货、广告归因、财务结算和跨境币种。不得仅凭字段名称把订单额、销售额、结算额和利润合并为同一口径。
- 同步设计基线：所有远端请求继续在后台执行，页面切换只读取 SQLite；长任务必须有运行状态、分阶段进度、可停止检查点、有限重试和明确错误信息。新增同步必须支持幂等 UPSERT 或“完整拉取成功后事务替换”，不能在分页中途失败时先删除旧缓存。
- 写操作安全边界：若下一阶段接入价格、库存、广告预算或活动开关等 WB 写接口，默认只开放读取和预检；正式写入前必须展示目标店铺、对象 ID、原值、新值和 API 依据，并要求用户二次确认。AI 建议不得自动执行远端写操作，每次写入必须留下独立操作日志及写后效果观察窗口。
- 测试与验收基线：每个接口至少补充响应解析、空响应、分页、限流/权限错误和幂等写库测试；正式交付依次执行 `cargo fmt --check`、`cargo check --locked`、相关 Rust 测试、`pnpm build`、正式 Tauri Release 构建、根目录 EXE 覆盖和可见页面验收。只有使用用户授权 Token 得到可核对的 WB 后台数据后，才可在本文档记录“真实 API 验收通过”。
- 下一步入口：开始开发前先读取 WB 官方最新 API 文档并锁定第一批业务需求；建议按“连接与权限诊断 → 商品/价格与库存 → 订单与退货 → 广告 → 财务结算 → 利润口径”分阶段推进，每次修改继续追加在本节之后，不覆盖历史记录。

### 2026-08-28：WB 左侧业务导航、订单图片、双利润与广告看板

- 导航重构：删除 WB 工作页顶部的“每日经营/订单/广告/仓库与库存/产品成本与运费/WB API 设置”标签条，全部移动到左侧导航；新增“本土利润”和“跨境利润”，WB 左侧现在包含经营总览、订单中心、广告运营、仓库与库存、商品与成本、本土利润、跨境利润、WB API 与汇率八个独立入口。
- 订单图片：WB Statistics 订单响应只有 `nmId` 等订单字段，不能直接提供商品图片。同步流程新增官方 Content API `POST https://content-api.wildberries.ru/content/v2/get/cards/list` 分页读取，按 `nmID` 把商品名、货号和首张 `photos[].big`（回退 `c516x688` / `square`）缓存到独立 WB SQLite 的 `product_cards` 表；订单查询通过本地 LEFT JOIN 返回图片，打开订单页面不会逐行请求远端 API。
- 权限与失败边界：官方商品卡片接口需要 Content 或 Promotion 类别 Token。图片同步失败时保留上次成功缓存，并在同步结果中显示具体错误；不会因缺图片删除订单，也不会用公开 CDN 规则猜测图片地址。
- 双利润模块：依据“商品与成本”中人工确认的仓库模式分流，`overseas` 进入 WB 本土/俄罗斯海外仓利润，`dongguan` 进入 WB 中国跨境仓利润；`auto/unknown` 不擅自归类，并在空结果中提示先确认仓库模式。本轮没有根据仓库名称模糊猜测经营模式。
- 利润周期：两类利润页面均提供日、周、月、季度切换，当前分别对应所选结束日前 1/7/30/90 天的滚动窗口。利润继续复用现有 WB 每日经营公式：销售额－商品级广告－暂估平台费－采购成本－物流成本；缺采购或物流的行显示“缺成本/物流”，不作为 0 利润混入汇总，页面同时展示已核算行数。
- 广告优化：在原商品级广告归因明细前新增与 Ozon 广告效果看板一致的折线交互，按日展示广告花费、归因销售额和 ROAS，并补充区间花费、归因销售、广告订单和 ROAS 指标卡。数据仍来自 WB `adv/v3/fullstats` 商品级缓存，没有用订单销售额伪造广告归因。
- 修改文件：`desktop-next/src/App.tsx`、`desktop-next/src/OperationsPages.tsx`、`desktop-next/src/types.ts`、`desktop-next/src/phase2.css`、`desktop-next/src-tauri/src/wb.rs`、本 Markdown。
- 测试：`cargo fmt --check`、`cargo check --locked`、`pnpm build` 通过；Rust 全库 37 项通过、0 失败、1 项真实 Ozon 浏览器测试按设计忽略。正式 Tauri Release 构建通过。
- 正式程序：Release 已覆盖根目录 `ozon-analytics-next.exe`，两者 SHA-256 均为 `59C515F580E9AF79B1FA41A4501FD08F68E014C461E1BB4D426277477CFF1DD0`。根目录程序成功启动并返回唯一窗口 `Ozon ERP`；准备继续点击 WB 页面验收时检测到用户正在操作同一窗口，按桌面控制安全规则停止自动输入，因此本轮不冒充完成页面级点击验收。
- 真实数据验收：必须由配置了 Content/Promotion 权限的 WB Token 执行一次“同步 WB API”，随后核对订单图片与 WB 后台商品一致；再为商品设置 `overseas` 或 `dongguan` 仓库模式，核对两类利润不会重复归属。没有获得真实 API 响应前，仅可确认源码、数据库迁移、测试、构建和启动通过。

### 2026-08-28：WB 利润页订单缺失诊断与 Finance 结算接口结论

- 原因证据：只读检查 `data-next/wb/wb_analytics.db` 后确认本地实际保存 96 条 WB 订单，日期覆盖 2026-07-27 至 2026-08-27；用户截图所选范围同步到 19 条。页面没有订单并非订单 API 未返回，而是 `product_costs` 当前为 0 行，旧前端只依据成本表里的人工仓库模式筛选利润行，导致全部订单被排除。
- 仓库证据：现有订单仓库名包含 `382818-Dongguan-15to30Days-All types of transport-All goods` 等明确 Dongguan 标识。后端本来已经按“人工配置优先；明确的东莞/Dongguan/广东标识归为跨境；明确俄罗斯名称归为本土；其余保持未知”解析模式，但旧返回结构没有把解析结果传给前端。
- 修复：`WbDaily` 新增 `warehouse_mode/warehouseMode`，后端将最终解析模式随每日行返回，利润页直接按该最终模式筛选。这样没有成本资料的明确东莞订单也能出现在跨境利润页；采购或物流仍保持缺失状态，不会按 0 成本虚增利润。含糊仓库名仍不自动猜测，操作员可在“商品与成本”中覆盖模式。
- 财务接口结论：WB 有与 Ozon 应计费用明细相近的结构化结算数据。当前官方入口为 Finance API 的销售报告列表和销售报告明细（`POST /api/finance/v1/sales-reports/list`、`POST /api/finance/v1/sales-reports/detailed/{reportId}`、`POST /api/finance/v1/sales-reports/detailed`）；旧 `GET /api/v5/supplier/reportDetailByPeriod` 已进入停用流程，后续不得新增依赖。
- 财务文件：WB Documents API 还可列出并下载会计文档；利润核算应优先接入结构化 Finance 明细并缓存到独立 WB SQLite，文档下载用于归档和人工对账，不应通过解析 PDF/Excel 代替正式字段口径。
- 当前边界：本轮修复利润页订单过滤并完成接口可行性确认，尚未把 Finance 销售报告写入 WB 同步流程。接入时必须保存报告 ID、业务日期、服务/扣费类型、金额、币种和原始行唯一键，采用分页完整成功后事务替换，并单独展示销售、佣金、物流、仓储、广告、罚款、补偿与调整，避免重复扣费。
- 修改文件：`desktop-next/src-tauri/src/wb.rs`、`desktop-next/src/types.ts`、`desktop-next/src/OperationsPages.tsx`、本 Markdown。
- 验证：`cargo fmt --check`、`cargo check --locked` 通过；Rust 全库 37 项通过、0 失败、1 项真实 Ozon 浏览器测试按设计忽略；`pnpm build` 通过。
- 正式程序：已通过 `desktop-next/scripts/build-tauri-release.cmd` 重建并覆盖根目录启动器；Release 与根目录 `ozon-analytics-next.exe` 的 SHA-256 均为 `2FAC9AA1A1967BFC0C199FC8BA02319A7791E39BA28A0D532FAE30957EF411F0`。

### 2026-08-28：WB 盈亏表缺成本行快捷补录

- 操作入口：WB 本土利润与跨境利润表中的未核算行新增“补充成本”按钮，并支持双击整行打开编辑窗口；完整行继续显示利润，不增加无意义操作按钮。
- 数据联动：编辑窗口直接读取和保存 `product_costs` 中与“商品与成本”模块相同的 `WbCost` 记录，没有创建第二套利润专用成本表。可补充采购成本、长宽高、重量、货号和仓库模式。
- 重新核算：保存调用既有 `save_wb_cost` 后重新读取 WB 设置、每日利润与成本列表，盈亏表和“商品与成本”同步更新。物流估算所需尺寸或重量仍缺失时，该行继续显示待补充，不会按 0 物流核算。
- 校验：成本、尺寸和重量只接受空值或大于等于 0 的有限数字；保存失败保留窗口并显示具体错误。
- 修改文件：`desktop-next/src/OperationsPages.tsx`、`desktop-next/src/phase2.css`、本 Markdown；`pnpm build` 通过。
- 正式程序：Tauri Release 已重建并覆盖根目录启动器；Release 与根目录 EXE 的 SHA-256 均为 `17A7724ADF82AEF19D03F840FFD98F9DA724C7A426A3796DFBBCA640D20A70F5`。
- 正式程序：Tauri Release 已重建并覆盖根目录启动器；Release 与根目录 EXE 的 SHA-256 均为 `73980B194B42608C51D9DC58693CDDA0D78ADC2BDA57BFBFDC6255D9DC3C359A`。

### 2026-08-28：WB 成本逐行保存按钮状态优化

- 保存按钮改为统一圆角操作样式，并增加未修改、保存修改、保存中、已保存四种明确状态；未修改行按钮弱化且禁用，避免重复提交和整列视觉噪声。
- 任一货号、采购成本、尺寸、重量或仓库模式发生变化后，仅对应 nmId 的按钮高亮。保存成功短暂显示绿色完成状态，失败则保留待保存状态并展示错误。
- 保存成功后立即重新读取 WB 每日利润和成本缓存，因此切换到本土/跨境盈亏表时可直接看到重新核算结果。
- 修改文件：`desktop-next/src/OperationsPages.tsx`、`desktop-next/src/phase2.css`、本 Markdown；`pnpm build` 通过。
- 正式程序：Tauri Release 已重建并覆盖根目录启动器；Release 与根目录 EXE 的 SHA-256 均为 `EE79087A5D564C4842DCAD9AEE17981CE2F96AD4113D77B46EE6A1393D75C4EC`。

### 2026-08-28：WB 商品与成本即时搜索

- “商品与成本”表格顶部新增搜索框，支持按 nmId、货号和仓库模式进行不区分大小写的包含匹配，并显示“匹配数 / 总数”。
- 搜索仅过滤已载入内存的 WB 成本列表，不访问远端 API、不重复读取 SQLite；清空输入后立即恢复全部记录。
- 筛选后编辑仍按不可变 nmId 更新原始成本集合，避免因筛选改变数组下标而误改其他商品；没有结果时显示当前搜索词对应的空状态。
- 修改文件：`desktop-next/src/OperationsPages.tsx`、`desktop-next/src/phase2.css`、本 Markdown；`pnpm build` 通过。

### 2026-08-29：Ozon 跨境缺成本明细与 WB 报告中心

- Ozon 明细：跨境店铺利润在原有“缺 N 个 SKU”汇总之外，新增“未完整核算 SKU 明细”。逐项显示货号、SKU、商品名、销量、采购成本、重量、单件跨境运费，并明确区分“采购成本未填写”“重量未填写”“重量超出跨境运费区间”和“历史佣金/收单费率不足”。利润仍不会把不完整商品按 0 成本计入。
- 补录边界：采购成本与重量继续由现有“产品成本”数据源维护；历史佣金/收单费率来自 Finance 结算样本，不能通过人工成本补录解决。页面明确提示两类处理方式，没有新建第二套成本表。
- WB 导航：左侧新增“报告中心”，对应 WB 卖家后台报告入口。当前周期的有效订单、销量、销售额、活跃 nmId、可用库存、退回途中库存和取消订单直接复用独立 WB SQLite 的真实缓存。
- 已接入报告：每周销售趋势分析、销量/实时销售概览、库存报告分别链接到现有经营总览、订单中心和仓库库存页面，避免复制指标和产生不同口径。
- 覆盖矩阵：商品评分、隐藏商品、扣款、退货和货物移动、品牌销售价格指数、错误标识符/IMEI 均在报告中心显示当前接入状态及缺口。取消订单不会冒充正式退货，库存退回途中不会冒充退货结算，未取得的数据不生成模拟值。
- 官方接口核对：隐藏/屏蔽商品可由 Seller Analytics `banned-products/blocked` 与 `banned-products/shadowed` 获取；扣款可由 `deductions`、`antifraud-details`、`goods-labeling` 获取；退货移动可由 `goods-return` 获取。上述接口需要 Analytics 权限并受独立限流约束，本节点只建立可见报告映射，尚未在普通“同步 WB API”中自动请求，避免一次经营同步触发低频报告接口限流。
- 修改文件：`desktop-next/src/App.tsx`、`desktop-next/src/OperationsPages.tsx`、`desktop-next/src/analytics.css`、本 Markdown。
- 验证：`pnpm build` 通过；正式 Tauri Release 与 NSIS 安装包构建通过。经过 NSIS bundle 标记后的 Release 与根目录 `ozon-analytics-next.exe` 均为 26,383,872 字节，SHA-256 `FB1D3E355CCA0F1B673E3CF06B6154276425E14F9F8DC06606E22FE51ED2D042`；安装包 `Ozon Analytics_1.0.0_x64-setup.exe` 为 6,548,010 字节，SHA-256 `4FE2C456179CC88AA8AE76FA15BE8F39872037B78F0D1860A322ED05064E0351`。

### 2026-08-29：GitHub 最新源码同步与正式安装包重建

- 源码同步：本地 `main` 以 fast-forward 从 `da5770c` 更新到 GitHub `origin/main` 的 `2ee5f36`，拉取前工作区干净，无冲突、无本地源码覆盖。
- 测试：`cargo fmt --check` 与 `cargo check --locked` 通过；Rust 全库 37 项通过、0 失败、1 项真实浏览器测试按设计忽略；竞品 Python 测试 5/5 通过；正式前端通过 `desktop-next/scripts/build-frontend.cmd` 完成 TypeScript 与 Vite 构建。受限沙箱中的 DPAPI 测试首次因系统凭据目录不可访问失败，在真实 Windows 用户上下文复测通过。
- 正式构建：使用唯一入口 `desktop-next/scripts/build-tauri-release.cmd` 生成内嵌前端的 Release，再使用 `desktop-next/scripts/build-tauri-package.cmd` 生成 NSIS；以 NSIS bundle 标记后的最终 Release 覆盖根目录 `ozon-analytics-next.exe`。
- 交付物：Release 与根目录 EXE 均为 26,394,624 字节，SHA-256 `BBC33FEA30997788550502990BD6D94016A93FB6D0FD0EE623E429371F645155`；NSIS 安装包为 6,548,257 字节，SHA-256 `D55CE7B14D38DCBB716A3A5DD1AC0A9A32C4D7C77786C4163A8EBB02A3D286A5`，并复制为 `release/Ozon-Analytics-1.0.0-Setup.exe`。
- 启动验收：根目录 EXE 启动 8 秒未退出，唯一窗口标题为 `Ozon ERP`，进程 `Responding=True`；测试进程随后关闭，避免锁定正式文件。未使用真实 Ozon/WB Token执行远端数据同步，因此本轮验收范围为源码、测试、正式构建、安装包和本地启动。

### 2026-08-29：跨境新店商品回填与 RUB/CNY 展示修复

- 真实数据诊断：安装版“王总一店”独立数据库已经有 10,000 条 Seller Analytics 日数据、228 条 Performance 数据和 3,182 条 Finance 数据，但 `products` 为 0。商品中心从 `products` 起查，因此错误显示无产品；Seller 数据并未丢失。现有销售缓存中共有 314 个不同 SKU，可恢复商品基础记录。
- 商品修复：每批 Seller Analytics 写入 `sales_daily` 时同步 UPSERT 商品 SKU 与名称；数据库初始化时再从历史 `sales_daily` 幂等回填 `products`，并记录 `seller_product_backfill_version=1`，保证历史扫描只运行一次。因此已同步但商品表为空的现有跨境店无需重新下载数据，下一次选择店铺即可恢复商品中心。目录 API 后续仍可补充货号、图片和商品 ID。
- 币种根因：前端根据跨境店类型显示 `¥`，但经营总览后端此前直接返回 RUB 原值，形成“只换符号、不换金额”。现在经营总览销售额、客单价、广告花费、广告销售额和趋势，以及订单金额、预估配送费、广告分析金额，统一按当前店铺 `cross_border_rub_per_cny` 在后端由 RUB 除以汇率后返回；销量、订单、CTR、ROAS、ACOS 等无量纲指标不换算。
- 样本复核：该店汇率为 14 RUB/CNY；最近 7 天原始销售 ₽50,992.00 应显示约 ¥3,642.29，原始广告费 ₽5,258.37 应显示约 ¥375.60，广告归因销售 ₽45,072.00 应显示约 ¥3,219.43。截图中的 ₽6,075.57 按同一汇率应为约 ¥433.97。
- 修改文件：`desktop-next/src-tauri/src/lib.rs`、本 Markdown。新增跨境金额换算回归测试；Rust 全库 38 项通过、0 失败、1 项真实浏览器测试按设计忽略；`cargo fmt --check`、`cargo check --locked` 和正式前端构建通过。
- 正式交付：Release 与根目录 EXE 均为 26,388,992 字节，SHA-256 `B143275C161E3BC61BDE260D7154EC2A624ABD905591F30414DF442CC0576109`；NSIS 安装包为 6,549,060 字节，SHA-256 `45ECB65D733566431895A633580A1FEF7A49D3BF1829E7BF2C495BBB6085D4BD`。

### 2026-08-30：经营总览与利润表自然月快捷选择

- 经营总览的趋势周期新增本月、上月和上上月三个快捷自然月，按钮直接显示具体年月，例如当前为 2026 年 8 月时显示 `2026年8月`、`2026年7月`、`2026年6月`。原“最近 7 天 / 最近 30 天 / 本季度”仍保留；选择自然月后，经营卡片与趋势数据统一使用对应起止日期。
- 月度盈亏在月份输入和上月/下月按钮之外增加相同的三个具体月份按钮；跨境店铺利润也增加相同按钮，替代含义不明确的“月利润”。日利润与周利润仍可使用。
- 日期口径：本月从当月 1 日统计至今天；已结束月份从 1 日统计至该月最后一天。日期使用本地时区构造，避免 UTC 转换导致月初/月末偏移；跨年时按钮包含年份，不会把同名月份混淆。
- 修改文件：`desktop-next/src/App.tsx`、`desktop-next/src/OperationsPages.tsx`、`desktop-next/src/phase2.css`、本 Markdown。
- 验证：TypeScript 类型检查和 Vite 正式构建通过；`cargo fmt --check`、`cargo check --locked` 通过；Rust 全库 38 项通过、0 失败、1 项真实浏览器测试按设计忽略。受限沙箱中的 DPAPI 测试因系统凭据目录不可访问失败，在真实 Windows 用户上下文复测通过。
- 正式交付：已使用 `desktop-next/scripts/build-tauri-release.cmd` 和 `build-tauri-package.cmd` 重建并覆盖。Release 与根目录 EXE 均为 26,388,992 字节，SHA-256 `5615B25EC4CA42C4275B259AE844689AE21A82A80766959BEC9E49396F5DE67F`；安装包为 6,551,278 字节，SHA-256 `430B092AF496DCE5C2CC744F29388704C36B1E9414A64AE72D40264E9B4BD6A2`，已复制为 `release/Ozon-Analytics-1.0.0-Setup.exe`。
- 启动验收：根目录正式 EXE 启动 8 秒未退出，窗口标题为 `Ozon ERP`，`Responding=True`，确认加载内嵌正式前端而不是 `localhost:1420`；验收进程随后关闭，避免锁定交付文件。
### 2026-08-31：经营周报广告费按数据层级去重

- 原因：Performance 同步同时保存 `sku=''` 的店铺级日汇总和 `sku<>''` 的商品级明细。广告看板已有“存在店铺级行时只取店铺级，否则回退 SKU 明细”的规则，但经营周报直接累加当天所有 `ad_daily` 行，导致两个层级重复统计。
- 数据证据：默认本土店 2026-08-25 店铺级广告费为 `51,602.00 ₽`，SKU 明细合计 `26,504.36 ₽`；旧周报显示的 `78,106.36 ₽` 正好是二者相加。2026-08-24 至 2026-08-30 正确店铺级合计为 `300,280.97 ₽`，旧周报 `459,597.42 ₽` 额外重复加入了 `159,316.45 ₽` SKU 明细。
- 修复：周汇总和本周每日明细均改为逐日选择广告数据层级：当天存在店铺级行时只统计 `sku=''`；不存在时才累计 `sku<>''`。广告花费与广告订单使用同一层级规则，避免金额已去重但订单仍重复。
- 缓存：分析详情指纹升级为 `analytics-v5-ad-level-dedup`，旧的周报缓存会自动失效并重新计算，无需删除数据库或重新同步远端 API。
- 验证：`cargo fmt --check`、`cargo check --locked` 与 `pnpm build` 通过；正式 Tauri Release 和 NSIS 构建通过。根目录启动器为 26,382,848 字节，SHA-256 `48D080A9D4F04EA81385297900ECB869256E88BEDC6B07031B38AC559C046736`；安装包为 6,549,813 字节，SHA-256 `21034954470A40F6D8337DA32CB16FBF9A3D7F3D9562D020D3F79C3714CB09A3`。
### 2026-08-31：新增产品分析决策模块

- 独立入口：Ozon“营销与洞察”导航新增“产品分析”，与经营总览内已有的单品/系列概览分离，专门用于日常投放、利润和库存决策。
- 固定周期：默认且最大只能选择前一完整自然日，不把当天未结束数据或“今日 0 单”纳入判断。广告与经营数据统一使用同一天；近 7 日销量只用于库存周转，近 3 日点击只用于样本阈值。
- SKU 字段：逐 SKU 返回曝光、点击、广告订单、广告销售额、广告费、总销量、总销售额、缓存价格与仓库库存。广告仅使用可精确归属 SKU 的缓存，或活动名称唯一匹配到一个 SKU 的店铺级活动，不按销售额比例猜测分摊。
- 漏斗：计算 CTR、CPC、CVR、CPA 和 ROAS，并依次诊断无曝光、CTR 偏低、CVR 偏低及 CPA 超过利润上限；界面同时显示三日点击样本量。
- 链接总盘：支持勾选两个或多个 SKU，统一汇总销量、销售额、广告费、广告销售额、ROAS 和库存，避免把变体迁移误认为链接增量。
- 利润上限：CPA 上限按“当前单价－采购成本－首程成本－历史 Finance 佣金/单件物流”计算；任一成本或历史费率缺失时显示“成本不完整”，不会把缺项按 0 计算。保本 ROAS 为单价除以广告前单位贡献利润。
- 库存与放量：库存天数为当前库存除以近 7 日平均销量；增量 ROAS 使用分析日相对前一日的广告销售额增量除以广告费增量。低于保本 ROAS、CPA 超限或不足 3 天/100 点击时给出回退或继续积累样本提示。
- 测试纪律：页面固定提示每次只改变预算、图片、价格中的一个；至少观察 3 天或累计 100 点击，周复盘优先看所选 SKU 的链接总利润与 TACOS。
- 修改文件：`desktop-next/src-tauri/src/insights.rs`、`desktop-next/src-tauri/src/lib.rs`、`desktop-next/src/ProductAnalysisPage.tsx`、`desktop-next/src/product-analysis.css`、`desktop-next/src/App.tsx`、`desktop-next/src/types.ts`、`desktop-next/src/bridge.ts`、本 Markdown。
- 验证：`cargo fmt --check`、`cargo check --locked`、`pnpm build`、正式 Tauri Release 与 NSIS 构建通过。根目录启动器为 26,410,496 字节，SHA-256 `3F2C070190E152F4B6052737CA1822C3ABA5FA95F6A188ECF45A71185DDD589D`；安装包为 6,551,349 字节，SHA-256 `BEC3A5056D1090C5BB124C57311F900CDA9ECE769B17811A5EFDC4A1E2F7281B`。
# 2026-08-31 产品分析评分看板

- 产品分析由宽表改为卡片与诊断看板：整体健康度、产品评分卡、五维评分、漏斗、利润安全线、库存压力和经营动作。
- 五维评分总分 100：流量 20、转化 20、利润 25、库存 15、放量 20；同时显示评分置信度，低样本或缺成本不会伪装成确定结论。
- 评分规则在页面公开展示；原始指标仍保留在对应看板中，便于复核评分来源。
# 2026-08-31 产品分析可解释性、SKU 关联与广告归属修复

## 2026-08-31 交互口径修正

- 产品评分卡只用于切换当前诊断 SKU，不再直接打开弹窗，避免浏览大量产品时被明细窗口打断。
- 顶部汇总指标、下方五维评分、漏斗、利润、库存与评分规则继续保留可解释明细入口。
- 本轮是把参考包的 P0 思路接入现有 Ozon ERP，不等同于完整实现参考包规划的独立决策系统。已经落地广告口径修复、同链接 SKU 持久关联、基础评分、置信度、规则版本、建议动作和证据查看；尚未完整落地 30 天状态引擎、变体迁移自动识别、集群 LSI、事件归因、边际增量实验和链接/变体双层投资分。

## 2026-08-31 P0 链接总盘与变体迁移

- 产品分析接口增加相邻两个完整 7 天窗口的 SKU 销量、销售额、广告费、广告订单和广告销售额，规则版本升级为 `product-analysis-v2.1.0`。
- 关联两个以上 SKU 后显示链接诊断看板和变体对比表，输出链接健康分、链接销量变化、各变体 7 天 ROAS、变体相对分和投资分。
- 变体迁移严格采用参考包阈值：目标变体销量下降至少 20%，链接合计销量下降不超过 10%，其他变体承接至少 60% 的损失销量。
- 投资分当前采用 `35% × 链接健康分 + 65% × 变体相对分`；相对分使用当前可得的销量、ROAS、五维经营分和库存覆盖。待类目基准与集群数据完善后，再加入类目绝对分和 LSI 修正。

## 2026-08-31 链接切换与利润安全线修复

- 点击已保存的链接组时，同时更新选中 SKU 集合和当前诊断 SKU，底部五维评分、经营评价及漏斗不再停留在上一个产品。
- 产品成本主口径是 CNY。产品分析在本土店中使用系统设置的 `local_rub_per_cny` 将 `unit_cost_cny` 换算为 RUB；不再错误地只读取旧字段 `unit_cost`。
- 利润安全线缺失时显示采购、头程、平台费/件的当前匹配结果，并提供“前往商品中心补成本”，自动带入货号搜索。
- XSB002（SKU 2550136937）核对结果：售价和 Finance 费率存在、采购成本保存为 `26.5 CNY`、头程为 `50.58 RUB`；旧实现未换算 CNY 成本，因此 CPA 上限和保本 ROAS 同时为空。

- 已读取 `ozon_product_optimization_codex_package/CODEX_START_HERE.md` 及资料包全目录，以 `result / evidence / confidence / rule_version / recommended_action` 作为产品分析解释结构。
- 修复 Performance Ads 的混合粒度：SKU 明细存在但订单/销售额为 0 时，若活动名称只能唯一匹配一个 SKU，保留 SKU 明细曝光/点击/花费，并使用活动级订单/销售额补齐；页面显示归属方式。
- 产品分析复用 `product_series` / `product_series_members`，支持选择两个或多个 SKU 建立持久链接关联，并一键重新选择整组进行链接总盘比较。
- 顶部指标、产品评分卡、五维评分、经营评价、漏斗、利润、库存和评分规则均可点击，下钻弹窗显示计分过程、原始分子/分母、数据来源、置信度和规则版本。
- 真实数据复核：跨境店 JZGJB02（2026-08-29）由 SKU 明细取得曝光 759、点击 17、花费 ₽55.86，由唯一活动级数据补齐广告订单 1、广告销售额 ₽1,536，修复后 ROAS 27.4973。

## 2026-08-31 产品分析周期口径 v2.2.0

- 产品分析不再以单个“分析日”作为经营判断口径，新增“周分析、月分析、自定义范围”三种模式。周分析为截止日及之前 7 个完整自然日；月分析为所选月份首日至最后一个已结束自然日；自定义范围由用户指定开始和结束日期。
- 所有模式排除当天未结束数据，并自动构造紧邻当前范围的等长前期。例如本期 7 天就比较之前 7 天，自定义 20 天就比较之前 20 天。
- 曝光、点击、广告订单、广告销售额、广告费、销量和销售额先按日取原始量再跨日汇总；CTR、CPC、CVR、CPA、ROAS 和增量 ROAS使用汇总后的分子/分母重新计算，禁止对每日比率求平均。
- 库存取周期末快照，库存覆盖天数使用“期末库存 ÷ 所选周期日均销量”；五维评分、置信度、诊断动作、顶部卡片、原始数据弹窗和链接变体比较全部跟随所选周期。
- 链接总盘与变体迁移不再固定写死 7 天，改为“本期所选周期 vs 紧邻等长前期”；页面会明确显示两个日期范围和天数。规则版本升级为 `product-analysis-v2.2.0`。
- 样本判断同时要求周期至少 3 个完整自然日且累计点击不少于 100，避免单日高点击或长周期低点击被错误标记为可判断。
- 验证：TypeScript/Vite 正式构建通过；Tauri Release 与 NSIS 安装包构建通过。新 Release 已另存为根目录 `ozon-analytics-next-new.exe`（SHA-256 `20F81C3701D36F8A3627A9DFA0BB2ACE0BC6727B6186DFCA9A1E53D79AACACEE`）；安装包 `release/Ozon-Analytics-1.0.0-Setup.exe` SHA-256 为 `A325B24CA8D5C94B6E0154DB3F16ED98B82E17DBAF96D854DD8A4F8BA2E59256`。根目录旧 `ozon-analytics-next.exe` 被另一个权限上下文中的常驻实例持续占用并自动重启，因此本轮没有谎报已覆盖，待该实例退出后再替换即可。

## 2026-08-31 产品决策引擎 v3.0.0

- 按参考包 `CODEX_START_HERE.md`、业务规则、数据模型、页面工作流和验收测试继续实现七项 P1/P2 能力。每个结论均带日期范围、证据、可信度、规则版本、建议动作、下一关口、停止条件和缺失数据。
- 30 天完整状态引擎：保留最近 30 个完整自然日，生命周期与趋势相互独立；趋势优先使用过去相同星期基线，要求连续 3 个完整日异常，并对低销量与历史覆盖不足返回“样本不足”。当前生命周期的有效日使用可核验的销售日；由于数据库没有历史逐集群可售快照，页面明确标为 30 天窗口内状态。
- 自动变体迁移：对所有已保存产品链接组自动扫描，不再要求用户先进入链接；阈值为目标变体下降至少 20%、链接下降不超过 10%、其他变体承接至少 60%。命中后在页面顶部主动提示，并可直接切换到对应链接。
- 双层模型：链接健康分由需求趋势、流量获取、链接转化、利润与供应稳定构成；信任/售后数据缺失时从已知权重归一计算且在证据中提示。变体增量分由转化、流量、单位利润、本地化、价格和库存组成，采用 `70% 绝对阈值 + 30% 链接内相对分`；最终投资分为 `35% 链接健康 + 65% 可信度修正变体增量分`。
- 集群 LSI：利用近 30 天真实集群订单占比与产品中心保存的集群配送/库存配置计算库存需求匹配度。当前数据源缺少逐集群实际库存、本地配送率、实际时效和可售率，因此只输出可计算的匹配分，并将 LSI 覆盖标记为 35%；不得用默认值补齐另外 65%，也不得据此自动放量。
- 事件归因：接入产品改价审计日志，并检测广告费日变化；期末库存为 0 时只记录当前快照，因缺少历史库存快照不推断断货起始日。周期内出现价格、促销、库存和预算中的多个变量时，实验标记为污染，不输出单一因果结论。
- 边际实验：使用所选周期与紧邻等长前期计算边际 CPA、边际 ROAS、边际 CVR 和广告弹性；广告费增长至少 10%、点击增长、链接订单增长低于 5% 且边际 CPA 超线时判定边际饱和，即使平均 ROAS 良好也停止继续加预算。
- 下一放量关口：默认只允许单次预算增加 10%，观察 48–72 小时；要求订单增长至少 8%、边际 CVR 保留基线 80%、边际 CPA 不超线、库存覆盖至少 21 天、不是变体迁移且可信度达到 60%。自动停止条件包括订单增长低于 5%、边际 CPA/ROAS 越线、边际 CVR 下滑、库存不足和第二变量介入。
- 构建验证：`pnpm build`、正式 Tauri Release 与 NSIS 安装包均通过。新启动器 `ozon-analytics-next-new.exe` SHA-256 为 `51D258AFA42B59BD6C1B54E282769106FF36B7D94A73E49AD4D23F8AB4C003DD`；安装包 `release/Ozon-Analytics-1.0.0-Setup.exe` SHA-256 为 `003B1235D8B24F2ABA43BB475FE7DB75611470C973006E1D7CD7074088C836BC`。
