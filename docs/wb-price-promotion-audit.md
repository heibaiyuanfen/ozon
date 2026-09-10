# WB 价格与活动中心：仓库审计与交付边界

## Existing Price Model

- 旧 WB 库只有订单成交金额与 `product_price_cache` 等局部缓存，不是价格事实模型。
- 新商品主数据 `pm_listings` 保存 WB `nm_id/chrt_id` 与店铺范围映射；本模块以 `shop_id + listing_id` 作为价格归属。

## Existing Promotion Model

- 原有 `advert-api` promotion 代码是广告活动，不是价格促销日历。
- 本模块新增促销日历只读同步，避免把广告活动与价格活动混为一谈。

## Existing Cost / Profit Model

- `pm_skus.purchase_cost` 是精确字符串成本；旧 WB 模块还有采购成本、尺寸与暂估物流。
- 税、仓储、广告分摊、完整佣金反推尚未形成统一事实，因此当前自动利润仅为已知成本提示；没有明确最低安全价时 Guard 返回 `insufficient_data`。

## Existing Discount / Batch / Validation

- 原项目没有可复用的 WB 价格批量写入组件。
- 商品主数据已有严格映射门禁、角色校验、事务与字符串金额校验模式，本模块沿用这些约束。

## Existing Audit / WB Client

- 复用 `wb_audit_logs`、`wb_sync_jobs`、加密 token 与 `active_token`。
- UI 只调用 Tauri application command；WB 请求只在 Rust adapter 中执行。

## Required Changes

- 数据库：追加价格 RAW、快照、规则、改价、改价明细、写入任务、活动及活动资格表。
- API：新增统一 `price_center` command，提供 dashboard、同步、规则、preview 和 execute。
- UI：WB 工作区新增“价格与活动中心”，包含总览、SKU、批量改价、安全价、活动、历史和任务。

## Current Delivery Boundary

- 已实现 P0 安全骨架、真实价格/活动同步、真实价格提交和平台任务 ID 保存。
- WB 写价采用官方 `POST /api/v2/upload/task`；成功响应仅标为待验证，不宣称最终平台价格已确认。
- 自动延迟验证、失败项明细回读/单项重试、活动商品资格明细和管理员 override UI 留待下一迭代；当前被安全地显示为不可用或待同步，不伪造成功。
