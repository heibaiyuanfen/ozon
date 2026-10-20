# 统一价格促销管理

2026-10-20。

- 活动目录：GET /v1/actions。
- 参与商品：POST /v2/actions/products；候选商品：POST /v2/actions/candidates。
- 加入、改价、统一价格退出：POST /v1/actions/products/update。
- POST /v2/actions/products/deactivate 仅保留优惠码专用移出；弹性加速、库存折扣、最大加速不再调用该接口。

退出普通统一价格活动：用户先填写明确的退出价格，确认该价格会影响商品其他活动。后端重新读取活动目录及完整参与清单，核对当前店铺、商品 Product ID、活动类型和最新 max_action_price，要求按两位小数提交的价格严格高于限价。平台响应 deactivated_product_ids 包含商品且回读清单不再包含商品，才显示已退出。拒绝、缺少确认或回读失败均不显示退出成功，也不自动重试写请求。

验证：官方活动目录及 /v2/actions/products 只读实测成功。测试样本商品1457096169，max_action_price=132.63 CNY，action_price=116 CNY；未改动该商品。

官方统一价格说明：https://seller.ozon.kz/media/news/edinaya-predelnaya-cena-menshe-ruchnoj-raboty-s-akciyami-cis/
官方迁移公告：https://dev.ozon.ru/start/563-Obnovlenie-metodov-raboty-s-aktsiiami-v-Seller-API-v2/
官方文档：https://docs.ozon.ru/api/seller/#tag/Promos

本次官方 Swagger 下载受到反机器人页面限制，未绕过；接口字段同时依据已有项目迁移记录与官方 API 真实只读响应核对。写入与退出端到端状态仍需用户在程序确认真实操作后验证。
