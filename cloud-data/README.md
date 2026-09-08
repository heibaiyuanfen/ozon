# A/B 数据库同步

- 每台电脑首次只执行一次：`powershell -ExecutionPolicy Bypass -File scripts/cloud-data-sync.ps1 -Action Configure -Slot A`（家里电脑改为 `B`）。身份写入 `%LOCALAPPDATA%\OzonERP\cloud-device.json`，不随 Git 仓库迁移。
- 两台电脑必须使用相同的 `OZON_CLOUD_PASSPHRASE` 或相同的 `%LOCALAPPDATA%\OzonERP\cloud-sync.key`。密钥绝不提交 GitHub。
- 上传本机数据：关闭程序后运行 `powershell -ExecutionPolicy Bypass -File scripts/cloud-data-sync.ps1 -Action Push`。
- 更新代码和本机数据库：关闭程序后运行 `powershell -ExecutionPolicy Bypass -File scripts/update-workstation.ps1`。
- A 只能读写 `cloud-data/devices/A`，B 只能读写 `cloud-data/devices/B`；传入相反槽位会立即拒绝。
- 每次恢复先在 `%LOCALAPPDATA%\OzonERP\backups` 备份当前 `data-next`，再校验远端包 SHA-256 和加密认证码。
