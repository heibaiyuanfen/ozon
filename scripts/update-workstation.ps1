param([switch]$SkipDatabase)
$ErrorActionPreference='Stop';$root=Split-Path -Parent $PSScriptRoot;$safeRoot=$root.Replace('\','/')
function Git([string[]]$Arguments){$output=& git -c "safe.directory=$safeRoot" -C $root @Arguments;if($LASTEXITCODE-ne 0){throw "Git 命令失败：git $($Arguments -join ' ')"};$output}
if(Git @('status','--porcelain')){throw '工作区存在未提交修改，拒绝自动更新代码'}
Git @('fetch','origin','main');Git @('pull','--ff-only','origin','main')
if(!$SkipDatabase){& (Join-Path $PSScriptRoot 'cloud-data-sync.ps1') -Action Pull -RepositoryRoot $root}
Write-Output '代码与本机绑定数据库均已更新。'
