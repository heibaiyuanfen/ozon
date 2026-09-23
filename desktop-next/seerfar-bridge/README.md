# Seerfar 浏览器辅助扩展（试验版）

此扩展只在 Ozon ERP 打开带一次性 `ozon-erp-capture` 标记的 Ozon 商品页时工作。它读取页面上**可见且可被网页 DOM 访问**的 Seerfar 面板文字，把当前 SKU 的字段回传到本机 `127.0.0.1` 临时端口；不会访问 Seerfar 账号、Cookie 或扩展私有存储，也不会向外部服务器上传。

手动安装：在装有 Seerfar 的 Chrome/Edge 中打开扩展管理页，启用开发者模式，选择“加载已解压的扩展程序”，选择本目录。安装和浏览器权限须由用户自己确认。然后先登录 Ozon 和 Seerfar，再在 ERP 的“竞品店铺拆解”导入 Excel，对商品点击“Seerfar 补全”。

限制：若 Seerfar 面板位于跨源 iframe、closed Shadow DOM，或者字段标签与当前中文标签不同，Chrome 的隔离机制会阻止此扩展读取。此时 ERP 保留 Excel 原值，补全不会显示成功；不应把缺失值记作零。当前实现是试验版，需在实际 Seerfar 页面上验收字段解析。
