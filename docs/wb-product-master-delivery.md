# Module 02 商品资料中心交付记录

日期：2026-09-09。范围：`02_PRODUCT_MASTER.md` 的商品主数据、同步、映射及批量工具；不是整个 WBerp 所有后续业务模块的完成声明。

## CHANGED FILES

- `desktop-next/src-tauri/src/product_master.rs`：商品领域、规范化、解析器、事务化 RPC、业务测试。
- `desktop-next/src-tauri/src/product_master_extra.rs`：增量迁移、查询、详情、编辑、权限、冲突复核、建议、历史引用。
- `desktop-next/src-tauri/src/product_import.rs`：CSV/XLSX 解析、校验预览、确认提交、导入审计与测试。
- `desktop-next/src-tauri/src/product_master_schema.sql`：基础商品表、唯一约束与索引。
- `desktop-next/src-tauri/src/product_worker.rs`：同步任务执行、分页、RAW、重试、取消、回收站、进程互斥。
- `desktop-next/src-tauri/src/wb_shop_center.rs`：数据库等待、同步排队事务、全量/增量模式、运行中取消。
- `desktop-next/src-tauri/Cargo.toml`：Windows 线程同步 API feature。
- `desktop-next/src/ProductMasterPage.tsx`、`desktop-next/src/product-master.css`：商品中心界面。
- 已有接入点：`lib.rs`、`App.tsx`；本次沿用其商品命令注册、工作线程与导航。
- 根目录 `ozon-analytics-next.exe`：按正式构建入口更新，结果见迁移日志。

## DATABASE CHANGES

数据库沿用 Module 01 的 `data-next/wb-v2/shop_api_center.db`。没有清空、迁移覆盖 Ozon 或旧 WB 的业务数据。

基础表：`pm_spus`、`pm_skus`、`pm_listings`、`pm_barcodes`、`pm_raw`、`pm_history`、`pm_migrations`。

本次扩展：

- `pm_metadata` 保存俄文内部名称、明确单位的尺寸重量、币种等扩展资料。
- `pm_imports` 保存待确认导入及提交状态；重复确认被拒绝。
- `pm_external_references` 以店铺、来源、外部记录键唯一保存原始引用及解析结果。
- Listing 增加 `listing_status`、`missing_fields`、创建/修改时间。
- 迁移在事务中执行，保留旧列和旧记录；重复运行不重复添加列。
- SKU 编码在组织内唯一，Listing 在店铺 + nmID + chrtID 内唯一。SKU 外键可对应多个店铺。
- 金额按十进制字符串保存，最多四位小数；不通过浮点计算采购成本。
- 未提供硬删除接口；归档保留 SKU 与历史映射。

## DOMAIN MODEL / DATA OWNERSHIP

内部 SKU ID 是核心身份；SPU 包含变体；MarketplaceListing 连接内部 SKU 与外部店铺尺寸，条码支持一对多。

ERP 拥有内部编码、中俄文内部名称、采购成本、内部品牌、供应商、内部类目及人工维护的属性与图片。WB 同步更新外部快照、外部品牌/类目/图片、标识和可观测的平台状态，不覆盖上述 ERP 字段。内部长度使用 cm，重量使用 kg；未知值留空。

## WB SYNC

Module 01 PRODUCTS 任务 → Content API → 完整响应先写 RAW → 每张卡片事务规范化 → 按尺寸创建/更新 Listing → 标识匹配与冲突复核 → 回写任务计数。

- 支持全量核对与基于上次成功游标的增量同步，不清空重建。
- 普通列表和回收站分别请求；回收站记录标记 inactive，保留内部关联。
- 使用 `updatedAt + nmID`、回收站 `trashedAt + nmID` 分页；拒绝不前进游标。
- 429、5xx 和传输错误退避重试，单次请求超时 30 秒，每段同步上限 30 分钟。
- 单卡片失败回滚该卡，保留响应 RAW 和其它成功卡片。任务显示成功/部分成功/失败。
- 取消在请求边界生效；同时打开多个本地程序不会重复启动同一数据目录的工作线程。
- 不能凭“本次增量没出现”推断商品已删除；不可观测的 blocked 状态不伪造。

官方字段核对来源：[Wildberries 商品 API 文档](https://dev.wildberries.ru/en/docs/openapi/work-with-products)。该文档明确普通列表不包含回收站商品，回收站需单独获取。

## MARKETPLACE SKU RESOLVER / MAPPING RULES

- 所有查找以店铺隔离，已存在 Listing、chrtID、条码为强标识。
- 未提供尺寸/条码时才使用 Vendor Code / nmID，且必须无多尺寸歧义。
- 提供的强标识彼此矛盾时返回 conflict，不任选一个 SKU。
- 同一 nmID 的多个尺寸不能因为其中一个已绑定，就把整张卡归给该 SKU。
- 自动匹配仅使用已有绑定、精确条码或单尺寸卡片的内部编码与 Vendor Code 精确相等。
- Vendor 精确匹配标记 high，强标识标记 exact；标题相似度只给人工建议，不自动绑定。
- 返回 `status / skuId / listingId / method / confidence`；一个 SKU 对应多个候选 Listing 时不编造唯一 listingId。
- 重复条码与 chrtID 冲突双向标记；标识纠正、统一绑定或忽略后重新核验。仍有不同 SKU 共用冲突标识时，解析器继续阻止自动归属。
- 重绑和解除绑定保存原因、前后值、操作者及时间。
- 未解析引用通过 `remember_reference` 保存，`reprocess` 重算并审计。后续模块必须调用此服务；本次不虚构尚未开发的订单/库存/财务表。

## API CHANGES

沿用 Tauri `product_master(action, payload)`，没有引入额外 HTTP 服务。

操作：`list/list_v2`、`detail/detail_v2`、`create_spu`、`update_spu`、`create_sku`、`update_sku`、`preview_bind`、`bind`、`unbind`、`ignore`、`bulk_update`、`summary`、`duplicates`、`suggest`、`resolve`、`remember_reference`、`reprocess`、`import_preview`、`import_commit`。

商品同步通过 `wb_shop_center('sync', {shopId,resourceType:'products',mode:'full'|'incremental'})` 创建任务。

## UI CHANGES

- 系列、内部 SKU、WB Listing、未匹配、冲突、批量工具六个入口。
- 创建/编辑/归档，SKU 创建时选择现有系列或同时创建新系列。
- 列表分页、搜索外部标识和内部 SKU、店铺/系列/状态/品牌/类目/供应商/数据质量筛选。
- SKU 与系列详情，变体结构、平台关联、属性、媒体、历史；后续业务显示待接入，不显示伪造价格库存。
- 绑定前搜索 SKU、人工建议、预览尺寸及冲突，再确认操作。
- 属性和图片用表单维护，平台原始字段在可展开区域保留。
- CSV/XLSX 商品导入和批量映射：上传、校验、预览、下载错误报告、明确确认、事务提交。
- 每次最多 5000 行 / 10 MB；XLSX 解压后最多 64 MB。CSV 模板可直接用 Excel 打开；外部标识建议以文本列保存。
- 批量分配系列/类目/供应商/状态，当前页导出，重复标识报告导出。
- 同步状态定时更新，保留加载/空数据/失败提示。

## PERMISSIONS

使用本地桌面部署的可信进程角色 `WBERP_LOCAL_ROLE`：默认 admin，支持 operator/viewer；前端传入 actor 或 actorRole 一律拒绝。

Admin 可以创建、编辑、归档、导入和映射；Operator 可读取、绑定、有限编辑和重算引用，不能更改编码、成本、供应商或状态；Viewer 只读。

这是本地单用户权限边界，不是多用户登录或远程服务认证系统。不能把环境变量角色当作多用户安全隔离方案。

## TEST RESULTS

截至正式构建前：商品模块 37 项测试通过、店铺中心 2 项测试通过；`cargo check --locked`、`pnpm build` 通过。新增前端文件 Prettier 检查通过；`cargo clippy --locked --lib` 成功，仍有其它现有模块的 19 条警告，新增商品/同步文件无 Clippy 警告。

测试覆盖：CRUD/归档、金额精度、编码唯一、事务回滚、RAW 店铺一致性、多店铺关联、多尺寸歧义、幂等、条码冲突及恢复、手工绑定/重绑/解除/忽略、外部状态与字段变化、字段所有权、权限、CSV/XLSX、映射导入、搜索分页、stale 筛选、历史引用重算。

包含 10,000 SKU / 20,000 Listing / 50,000 Barcode 的数据库夹具，验证搜索结果和分页。它是功能规模检查，不代表生产环境压测 SLA。

最终测试、lint、构建、EXE 校验与可见验收结果追加在 `PROJECT_ARCHITECTURE_AND_MIGRATION.md`。

## KNOWN LIMITATIONS

- 价格、库存、订单、广告、财务、采购仍属于后续业务模块；Module 01 中其它资源的完整执行器不因此完成。
- 不提供商品发布/写回 WB、AI 标题/属性、自动类目映射；这些是规范中的后续或暂缓范围。
- 图片使用 URL，不包含本地文件上传到云端的媒体存储服务。
- 本地单组织/单用户模式；没有新增多用户登录系统。
- 归档商品仍可解析历史引用；新绑定拒绝归档 SKU。
- 全量/增量同步无法证明 WB 未公开的 blocked 状态；保留 unknown/数据缺失信息。
- SPU 选择器和详情变体列表目前上限 500 项；大规模 SKU/Listing 主列表使用服务端分页。

## NEXT MODULE DEPENDENCIES

`03_PRICE_PROMOTION.md` 可复用 `shop_id`、`sku_id`、`listing_id`、MarketplaceListing、`product_master::resolve` 及未解析引用重处理接口。不要另建 nmID 作为内部 SKU 主键，不要用价格、订单或广告模块自行猜测尺寸归属。
