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

### 更新记录模板

```markdown
### YYYY-MM-DD：更新标题

- 目的：
- 修改模块/文件：
- 数据口径或数据库变化：
- 缓存与性能影响：
- 测试和构建结果：
- 尚存风险/下一步：
```
