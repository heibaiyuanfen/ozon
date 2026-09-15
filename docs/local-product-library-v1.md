# 本地产品库 v1 接口契约

当前通过 Tauri `product_master` 命令调用，`action` 固定为 `upload_product_v1`。迁移云端时可将同一 payload 映射为 `POST /api/v1/products/upsert`。

## 请求

```json
{
  "apiVersion": "v1",
  "requestId": "desktop-20260915-0001",
  "product": {
    "product_code": "PROD-001",
    "name": "产品名称",
    "description": "产品描述",
    "brand": "品牌",
    "category": "类目",
    "supplier": {
      "name": "1688 供应商",
      "purchase_url": "https://detail.1688.com/example"
    },
    "costs": {
      "purchase_cost": "23.50",
      "domestic_shipping": "2.00",
      "label_fee": "0.50",
      "packaging_fee": "1.00",
      "other_cost": "0.00",
      "currency": "CNY"
    },
    "package": {
      "weight_kg": "0.35",
      "length_cm": "20",
      "width_cm": "15",
      "height_cm": "8"
    },
    "images": [
      { "url": "https://example.com/1.jpg", "sort_order": 1 }
    ],
    "attributes": { "color": "黑色" }
  },
  "shopTargets": [
    { "shopId": "shop-a", "offerId": "A001" },
    { "shopId": "shop-b", "offerId": "A001" }
  ]
}
```

`requestId` 是幂等键：相同请求号重复调用时返回首次结果，不重复写入。数据库强制 `UNIQUE(shop_id, offer_id)`，因此不同店铺可以使用相同货号，同一店铺不可重复。整个请求使用单一事务，任一店铺冲突时全部回滚。

## 成功响应

```json
{
  "success": true,
  "apiVersion": "v1",
  "requestId": "desktop-20260915-0001",
  "data": {
    "productId": "internal-uuid",
    "shopTargets": [
      { "id": "plan-uuid", "shopId": "shop-a", "offerId": "A001", "status": "ready" }
    ]
  }
}
```

货号冲突时返回包含 `DUPLICATE_OFFER_ID` 的错误，不会覆盖原记录。
