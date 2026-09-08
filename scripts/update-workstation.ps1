param([switch]$SkipDatabase)
$ErrorActionPreference='Stop';$root=Split-Path -Parent $PSScriptRoot
if(git -c "safe.directory=$root" -C $root status --porcelain){throw '工作区存在未提交修改，拒绝自动更新代码'}
git -c "safe.directory=$root" -C $root fetch origin main;git -c "safe.directory=$root" -C $root pull --ff-only origin main
if(!$SkipDatabase){& (Join-Path $PSScriptRoot 'cloud-data-sync.ps1') -Action Pull -RepositoryRoot $root}
Write-Output '代码与本机绑定数据库均已更新。'
