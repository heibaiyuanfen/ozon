# Ozon Analytics 本地安装说明

本版本是本地桌面软件，不需要连接自建云端服务器。

- 程序、界面和 SQLite 业务数据库均在本机运行。
- 安装版首次启动会在 `%LOCALAPPDATA%\com.ozonanalytics.desktop\data-next` 创建本地数据库。
- 安装包中的数据库模板为空，不包含开发者或其他用户的店铺数据、API 密钥和业务记录。
- Ozon、飞书和 Wildberries 同步功能会按用户配置访问相应平台的官方 API；不配置时仍可打开软件并使用本地功能。
- API 密钥保存在本机数据库中，敏感字段使用 Windows DPAPI 加密。

推荐普通用户运行 `Ozon Analytics_1.0.0_x64-setup.exe` 完成安装。便携包解压后可直接运行，但业务数据仍保存在上述本机用户目录，而不是写入压缩包目录。

卸载软件不会自动上传数据。删除本地业务数据前，请先备份 `%LOCALAPPDATA%\com.ozonanalytics.desktop\data-next`。
