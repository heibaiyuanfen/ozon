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
