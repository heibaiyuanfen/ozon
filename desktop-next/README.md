# Ozon Analytics Next

Tauri + React + Rust 桌面客户端，直接复用旧版 `data/shops.json` 和每个店铺的 SQLite 数据库。已包含经营健康度、销售/销量与广告趋势、订单与配送估算、FBS 时效预警、产品全参数成本匹配及导出、库存、广告、利润报告、AI 分析和竞品跟踪。

## 直接运行

直接双击正式发布程序，不依赖 localhost，也不需要先运行 Python 或网页服务。

## 重新构建

双击 `scripts\build-tauri-release.cmd`。构建产物位于：

`src-tauri\target\release\ozon-analytics-next.exe`

开发模式可运行 `scripts\dev-tauri.cmd`；仅执行浏览器预览只会看到模拟数据，真实本地数据库仅在桌面窗口读取。
