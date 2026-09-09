# Module 02 — 开发前审计

| 要求 | 当前证据与决定 |
|---|---|
| Existing Product Model | 旧 WB product_cards 以 nm_id 为主键；仅作为后续迁移来源，不直接复用主键。 |
| Existing SKU Model | Ozon products 使用平台 SKU；新 ERP SKU 必须独立建模。 |
| Existing Series/SPU Model | insights.rs 的 product_series 属于单 Ozon 店铺；不跨平台复用成员关系。 |
| Existing Marketplace Listing Model | 没有内部 SKU 与多店铺 Listing 桥梁，需要新增。 |
| Existing Category Model | 存在 Ozon 上品类目缓存；WB 外部类目和 ERP 类目必须分开。 |
| Existing Supplier Model | 合同包含乙方资料，没有统一供应商主档；本阶段供应商关联保留为空。 |
| Existing Image/Media Model | WB 商品卡片仅存一个 image_url；需要保留完整外部媒体快照与内部媒体边界。 |
| Existing Import System | 已有 calamine Excel 读取，缺少主数据预览提交机制。 |
| Existing Search/Filter Components | 复用 React/Tauri、表格与筛选视觉，查询必须后端分页。 |
| Required Reuse | Module 01 数据库、店铺、DPAPI 凭据、SyncJob/Run、审计。 |
| Required Extension | 商品执行器、RAW、Normalizer、统一 Resolver、权限来源。 |
| Required Migration | 版本化追加新表，旧 WB/Ozon 表不自动转换或清空。 |

## 已确认的依赖缺口

Module 01 的 pending 任务没有 worker，不能视为同步已完成；actorRole 来自调用 payload，不能视为可信 RBAC。商品模块上线前必须修正这些依赖。商品卡片按尺寸产生 Listing，nmID 不是尺寸唯一键，多个 chrtID 共享 nmID 属于正常情况。

## 官方接口依据

https://dev.wildberries.ru/docs/openapi/work-with-products

使用 POST /content/v2/get/cards/list，保存 RAW 后规范化，按 ascending=true 与 updatedAt/nmID cursor 分页。正常列表未出现某商品不代表已删除，不据此把缺席商品归零或下架。
