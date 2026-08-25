# 新版迁移校对与补齐记录（2026-08-25）

## 本轮范围

本轮逐项复核 `ozon_app/` Python 旧版与 `desktop-next/` React + Tauri/Rust 新版，覆盖经营报表、Finance、商品图片、库存、广告、WB、竞品采集和前端构建性能。

## 逐项结论

| 项目 | 新版实现与口径 | 本轮结果 |
| --- | --- | --- |
| 每日产品、产品利润、周报、跨境利润 | 销量/销售额来自 Seller 缓存；广告活动总额只读取 `sku=''`；采购成本按店铺汇率换算；跨境利润使用历史 SKU 佣金率并保留 Finance 已结算对账列；FBP、RFBS、WHD 从 Finance `delivery_schema` 分类 | 已完成代码对照；保留经营预估与已结算两套口径，不互相覆盖 |
| Finance SKU 归属 | 仅 `items[]` 中恰好一个唯一 SKU 时精确归属；多 SKU 记录和店铺级记录保留为未分摊 | 修复 Rust 同步中“多 SKU 回退到顶层 SKU”的错误；与 Python `_normalize_finance_operation` 一致 |
| 商品图片和订单图片 | Seller 商品目录与商品详情分批补全 `products.image_url`；订单查询按 SKU 连接图片；React 使用懒加载并在失败时隐藏损坏图片 | 已完成并核验调用链 |
| 库存字段 | `/v1/analytics/stocks` 保存仓库级可售、在途、已申请等字段；`/v4/product/info/stocks` 独立保存后台总库存和预留；页面分列显示来源不同的数值 | 已完成并核验，未把后台总量和可售量混成一个字段 |
| 广告 SKU 历史 | 活动总额与 SKU 明细使用相同表但不同 `sku` 唯一键，汇总防止重复计费；产品 SKU 接口仅增量保存接口允许的最近日期，历史缓存不删除 | 已完成 API 能力内覆盖；历史日期无精确明细时明确为空，不伪造分摊 |
| 大型图表 | 表格分页；ECharts 改为 Core 按需注册 Line/Bar/Pie 与必要组件；Rollup 分离 React、图表、图标和通用依赖 | 完成；主入口由约 1.45 MB 降至约 124 KB，最大图表块约 418 KB，消除 500 KB 警告 |
| WB 模块 | 独立 SQLite、Token、订单、商品级广告、成本、利润、仓库目录、同步和飞书；新增订单、广告、库存独立页签；库存接入 2026 新 Analytics `stocks-report/wb-warehouses` 接口 | 模块化和代码接入完成；无 Analytics 权限时保留上次库存缓存并明确提示，不回退到废弃接口 |
| 竞品验证页 | 直连成功时自动采集；失败时可打开系统浏览器完成验证，用户将商品页另存 HTML 后手动导入；不读取 Cookie 或浏览器凭证 | 完成安全恢复流程 |

## 关键修改

- `desktop-next/src-tauri/src/lib.rs`
  - Finance 多 SKU 保持未分摊。
  - 竞品 HTML 解析复用同一入口。
  - 新增打开浏览器、导入验证后 HTML 命令。
- `desktop-next/src-tauri/src/wb.rs`
  - 新增 WB 订单、广告、仓库和新 Analytics 库存明细查询与缓存。
- `desktop-next/src/OperationsPages.tsx`
  - 新增竞品验证页恢复 UI。
  - WB 工作区拆分每日经营、订单、广告、仓库与库存、成本、设置页签。
- `desktop-next/src/charts.ts`、`vite.config.ts`
  - ECharts 按需加载及 Rollup 手工分块。

## 数据安全和外部写入

- 竞品验证流程不提取浏览器 Cookie、登录态或密码，只读取用户明确选择的本地 HTML。
- Finance 多 SKU 费用不按销售额、件数或其他规则强行分摊。
- WB 库存不调用已废弃的 `supplier/stocks` 接口，也不把仓库目录伪装成库存数量。
- 本轮没有执行 Ozon、WB 或飞书远程写操作。

## 验证记录

- `pnpm build`：通过。
- 构建分块：入口约 124 KB，React 约 203 KB，通用依赖约 176 KB，图表约 418 KB。
- `cargo check`：通过。
- `cargo test --lib`：9 项中 8 项通过；唯一失败是 Codex 沙箱账户缺少可用 Windows DPAPI 用户密钥上下文，业务公式、迁移、上品与 WB 测试均通过。
- Tauri Release：构建通过，生成 `src-tauri/target/release/ozon-analytics-next.exe`。
- 窗口冒烟：新构建进程保持运行，标题为 `Ozon ERP`，Windows 报告 `Responding=True`。

## 保留风险

1. WB 新库存接口要求 Personal/Service Token 具备 Analytics 权限；无权限时同步其他模块仍可完成，库存保留上次成功缓存并显示原因。
2. Ozon Performance 的商品级 SKU 明细受接口可查询日期限制，无法通过客户端补造更早历史。
3. Finance 未分摊项目保留在店铺级对账中；只有获得可靠业务唯一键后才能归属 SKU。

## 2026-08-26：WB 经营看板与广告空缓存诊断

- 本地核验结果：WB 数据库已有订单 18 行、库存 12 行，但商品广告缓存 `ad_daily` 为 0 行；前端显示 0 不是图表渲染错误。
- WB 首页改为与 Ozon ERP 一致的信息架构：实时销量、广告表现双摘要卡，业绩指标条，销售/广告/利润折线趋势，经营健康度与同步状态。
- 移除销售额柱状图，改用带面积背景的销售额折线、独立右轴广告折线和虚线利润趋势，修复图例拥挤问题。
- 广告为空且订单存在时，页面明确提示检查 Token 的“推广”权限，不再只显示空白表。
- WB 同步结果新增广告活动数和商品广告行数；活动数为 0 时提示 Token 权限或活动状态范围。
- 广告活动 ID 解析兼容 `advertId`、`advert_id`，以及带 `changeTime` 的 `id` 响应结构。
- 复现截图进一步确认：活动列表成功返回 31 个活动，但旧解析器把 Fullstats 响应写死为 `days[].apps[].nm[]`，因此“活动可读取”不等于“商品广告已落库”。
- 商品广告解析改为携带活动 ID 和日期的递归语义解析，兼容 `nmId` / `nmID` / `nm_id`、直接商品节点及 WB 的多种金额字段别名。
- 同步结果拆分显示“广告活动、统计活动、商品广告”三个计数；若仍为空，会明确指出是统计接口未返回状态 7/9/11 活动，还是响应有活动但缺少商品层，并显示安全的顶层字段摘要（不记录 Token）。
- 2026-08-26 新 Release 已通过前端构建、`cargo check` 和 Tauri release 编译。

## 2026-08-26：月度盈亏缺成本 SKU 快速编辑

- “缺成本 SKU 明细”支持双击整行或点击“编辑成本”打开编辑窗口。
- 窗口预填已有采购成本、头程、长宽高、重量和备注，补单个缺失项时不会清空已有资料。
- 保存复用商品中心的 `product_costs` 唯一成本台账，并校验成本、尺寸和重量不得为负数或非法数字。
- 保存成功后立即刷新缺成本 SKU 列表及当前月度盈亏；成本资料补齐的 SKU 自动从缺失列表移除。
- 前端构建、`cargo check` 与 Tauri Release 均已通过。

## 2026-08-26：约仓计划切换卡顿修复

- 根因是供应单分页、详情、时段与预约接口使用同步 HTTP，并包含限频退避等待；同步命令执行期间会占用 Tauri 命令线程。
- 供应单列表、时段查询和预约提交均改为 `spawn_blocking` 后台任务，Ozon API 等待和重试不再阻塞窗口事件循环。
- 约仓页面增加卸载保护；切换到其他模块后，旧请求完成时不会再向已卸载页面回写状态或触发额外渲染。
- 前端构建与 `cargo check` 已通过。
