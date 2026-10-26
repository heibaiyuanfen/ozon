# WB 经营工作台（2026-10-24 重建）

WB 主导航已替换为经营总览、订单中心、退货中心。旧经营、广告、价格活动、利润、库存等页面不再挂载。店铺连接以弹窗提供，保留已有加密凭据和历史数据库，不删除原始业务数据。

## 使用方法

1. 切换 WB 工作区，在右上角选择店铺。跨境店没有接入时，点击“店铺连接”，填写新店铺名称和 API Token。更新已有连接时名称留空。
2. 选择日期和海外仓，点击“同步订单与退货”。单次订单创建日期最多 30 天，按莫斯科日期统计。FBS、DBS 分别读取，失败分别显示且保留原缓存；同步中可看到已经完成的数据源。
3. 经营总览提供订单量、扣除取消/拒收后订单额、待备货及待取退货数、每日趋势、逐日明细、履约状态和分仓待办。订单额不是销售结算收入或利润；未返回 RUB 金额的订单显示缺失数量，不做汇率猜测。
4. 订单中心支持日期、仓库、FBS/DBS、状态、订单号/货号搜索和 CSV 导出。等待备货、已经备货、配送中、等待签收、已签收、退货/拒收、已取消、状态待核实分别展示。
5. “标记已备货”是本地协同标记；平台配送、签收、取消等状态优先。此版本只读取 WB，不提交确认、发货、退款审批等平台操作。
6. 退货中心同时读取实物退货记录和买家退货申请，保留原始退货编号、原订单号及 SRID。平台退货字段与订单 ID/RID 精确匹配仓库；不按同款商品、同日期猜测仓库，未匹配时在“取件/关联仓库”中指定。
7. 可以设置取件地址、截止日期、备注，勾选“已核实实物到达”加入取件提醒，取件后勾选“我已取件”。这些记录独立保存在本机，后续同步不覆盖。已完成的平台记录不会继续提醒取件。
8. 总览和订单页顶部始终显示所选仓库未完成的取件待办，跨日期显示以免漏取。退货页默认显示全部缓存，不受日期筛选限制，可取消勾选以按日期查看。
9. 可开启每小时自动同步；程序须停留在 WB 工作区。自动同步订单按当前最近 7 天执行，非后台系统定时器。程序关闭时不会发送系统通知。

## 数据源

| 资源 | API | 权限 |
| --- | --- | --- |
| 卖家仓库 | GET marketplace-api.wildberries.ru/api/v3/warehouses | Marketplace |
| FBS 订单与新订单 | GET /api/v3/orders；GET /api/v3/orders/new | Marketplace |
| FBS 当前状态 | POST /api/v3/orders/status | Marketplace |
| DBS 订单与新订单 | GET /api/v3/dbs/orders；GET /api/v3/dbs/orders/new | Marketplace |
| DBS 当前状态 | POST /api/v3/dbs/orders/status/info；404 时尝试 /api/marketplace/v3/dbs/orders/status/info | Marketplace |
| 实物退货 | GET seller-analytics-api.wildberries.ru/api/analytics/v1/item-returns | Analytics |
| 退货申请 | GET returns-api.wildberries.ru/api/v1/claims | Buyers returns |

官方参考：[FBS](https://dev.wildberries.cn/docs/openapi/orders-fbs)、[DBS](https://dev.wildberries.ru/openapi/dbs)、[实物退货报告](https://dev.wildberries.cn/docs/openapi/reports)、[退货申请](https://dev.wildberries.cn/docs/openapi/customer-communication)。DBS 官方页面本次抓取受限，代码提供新旧路径兼容，真实跨境店接口尚未联调，不视为验证成功。

退货申请 API 仅提供近 14 天申请，持续同步后本机可保留更早历史。订单接口历史范围受平台限制，首次接入前的在途 DBS 订单能否完整回补需以该店铺平台响应核对。没有有效平台状态的记录显示“状态待核实”。返回到买家取货点的订单 `ready_for_pickup` 对应“等待签收”，不会误用为卖家退货取件提醒。

实物退货接口使用当前 item-returns，不使用官方标注于 10 月 26 日停用的 goods-return。实物退货分页按一分钟间隔读取，可能需要等待；不要在同步未完成时关闭程序。各接口权限或限流错误会写入同步结果。

## 数据与测试

新表 `wb_ops_rows`、`wb_ops_sync` 位于原 `data-next/wb-v2/shop_api_center.db`，按店铺、资源类型、平台编号隔离；原始平台字段与本地协同字段分开保存。没有自动修改仓库分配或库存的操作。

测试覆盖平台终态优先、FBS/DBS 状态映射、退货申请不能直接变成取件提醒、未来截止日期、精确仓库关联、跨店隔离、同步保留本地标记以及莫斯科日期转换。

## 2026-10-24 退货漏读修复

发现旧版的订单日期 10/18—10/24 同时传给实物退货接口，无法读到截图所示 9/24、10/02、10/12 的较早退货；同时旧版缺少销售/退货统计中的买家退货数据源。

现在订单仍按所选日期同步，实物退货和买家退货独立回补最近 90 天。新增 `GET https://statistics-api.wildberries.ru/api/v1/supplier/sales?dateFrom=...&flag=0`，只接收 `saleID` 以 R 开头的退货事件，保留 saleID、SRID、退货发生日期及平台仓库名，正常销售 S 记录不作为退货。按 lastChangeDate 分页至空响应，分页之间遵守一分钟间隔，已有页在等待期间可见。

退货中心新增买家退货、退回卖家、退货申请三个来源筛选和独立计数。买家退货不自动判定为实物已到达；仍需实物报告或人工核实后提醒取件。不同来源记录不直接相加为物理件数。退货申请 API 仍只提供近 14 天，界面说明已区分。

本机真实跨境店只读核对 2026-07-27—2026-10-24：实物退回 64 条，销售统计中买家退货 8 条。已回补本机对应店铺缓存，保留原有本地标记；回补前快照保存于 outputs/wb-return-diagnosis/before-backfill.json。官方原始响应仅保存在本机 outputs/wb-return-diagnosis/live.json，不包含 Token，不作为公开示例发布。

增加独立退货窗口、只采集 R 记录、幂等去重、不误报可取件及真实接口响应回归验证。真实响应测试使用内存数据库，确认截图三个日期的买家退货均能被生产解析逻辑读取。
