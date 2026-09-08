param(
  [Parameter(Mandatory=$true)][ValidateSet('Configure','Push','Pull','Status')][string]$Action,
  [ValidateSet('A','B')][string]$Slot,
  [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot)
)
$ErrorActionPreference='Stop'
$configDir=Join-Path $env:LOCALAPPDATA 'OzonERP'
$configFile=Join-Path $configDir 'cloud-device.json'
$keyFile=Join-Path $configDir 'cloud-sync.key'
$dataDir=Join-Path $RepositoryRoot 'data-next'

function Read-Config { if(!(Test-Path -LiteralPath $configFile)){throw '本机尚未绑定数据库槽位，请先运行 Configure -Slot A 或 B'}; Get-Content -Raw -LiteralPath $configFile|ConvertFrom-Json }
function Bytes([string]$s){[Text.Encoding]::UTF8.GetBytes($s)}
function Get-Key {
  if($env:OZON_CLOUD_PASSPHRASE){return $env:OZON_CLOUD_PASSPHRASE}
  if(Test-Path -LiteralPath $keyFile){return (Get-Content -Raw -LiteralPath $keyFile).Trim()}
  throw "缺少同步密钥：请设置 OZON_CLOUD_PASSPHRASE，或将相同密钥保存到 $keyFile"
}
function Encrypt-File([string]$source,[string]$target,[string]$password){
  $salt=New-Object byte[] 16;$iv=New-Object byte[] 16;$rng=[Security.Cryptography.RandomNumberGenerator]::Create();$rng.GetBytes($salt);$rng.GetBytes($iv)
  $kdf=New-Object Security.Cryptography.Rfc2898DeriveBytes($password,$salt,200000,[Security.Cryptography.HashAlgorithmName]::SHA256);$keys=$kdf.GetBytes(64)
  $aes=[Security.Cryptography.Aes]::Create();$aes.Key=$keys[0..31];$aes.IV=$iv;$aes.Mode='CBC';$aes.Padding='PKCS7';$plain=[IO.File]::ReadAllBytes($source);$enc=$aes.CreateEncryptor().TransformFinalBlock($plain,0,$plain.Length)
  $body=(Bytes 'OZONDB1')+$salt+$iv+$enc;$h=New-Object Security.Cryptography.HMACSHA256(,$keys[32..63]);$mac=$h.ComputeHash($body);[IO.File]::WriteAllBytes($target,$body+$mac)
}
function Decrypt-File([string]$source,[string]$target,[string]$password){
  $all=[IO.File]::ReadAllBytes($source);if($all.Length-lt 72-or [Text.Encoding]::UTF8.GetString($all[0..6])-ne 'OZONDB1'){throw '云端数据包格式无效'}
  $salt=$all[7..22];$iv=$all[23..38];$cipher=$all[39..($all.Length-33)];$mac=$all[($all.Length-32)..($all.Length-1)];$body=$all[0..($all.Length-33)]
  $kdf=New-Object Security.Cryptography.Rfc2898DeriveBytes($password,$salt,200000,[Security.Cryptography.HashAlgorithmName]::SHA256);$keys=$kdf.GetBytes(64);$h=New-Object Security.Cryptography.HMACSHA256(,$keys[32..63]);$expected=$h.ComputeHash($body)
  if(-not [Security.Cryptography.CryptographicOperations]::FixedTimeEquals($mac,$expected)){throw '密钥错误或数据包已损坏'}
  $aes=[Security.Cryptography.Aes]::Create();$aes.Key=$keys[0..31];$aes.IV=$iv;$aes.Mode='CBC';$aes.Padding='PKCS7';$plain=$aes.CreateDecryptor().TransformFinalBlock($cipher,0,$cipher.Length);[IO.File]::WriteAllBytes($target,$plain)
}
if($Action-eq'Configure'){
  if(!$Slot){throw 'Configure 必须指定 -Slot A 或 -Slot B'};New-Item -ItemType Directory -Force -Path $configDir|Out-Null
  @{slot=$Slot;machine=$env:COMPUTERNAME;configuredAt=(Get-Date).ToUniversalTime().ToString('o')}|ConvertTo-Json|Set-Content -LiteralPath $configFile -Encoding UTF8
  Write-Output "已将 $env:COMPUTERNAME 固定绑定为数据库 $Slot";exit
}
$cfg=Read-Config;$slotName=[string]$cfg.slot;if($Slot-and$Slot-ne$slotName){throw "本机已绑定 $slotName，拒绝访问数据库 $Slot"}
$deviceDir=Join-Path $RepositoryRoot "cloud-data/devices/$slotName";$package=Join-Path $deviceDir 'latest.ozondb';$manifest=Join-Path $deviceDir 'manifest.json'
if($Action-eq'Status'){[pscustomobject]@{Machine=$cfg.machine;Slot=$slotName;Config=$configFile;Package=$package;PackageExists=(Test-Path $package);LastPackage=if(Test-Path $package){(Get-Item $package).LastWriteTime}else{$null}};exit}
if(Get-Process 'ozon-analytics-next' -ErrorAction SilentlyContinue){throw '请先关闭 Ozon ERP，再同步数据库，避免复制或恢复写入中的 SQLite 文件'}
$work=Join-Path ([IO.Path]::GetTempPath()) ("ozon-cloud-"+[Guid]::NewGuid());New-Item -ItemType Directory -Path $work|Out-Null
try{
 if($Action-eq'Push'){
  $payload=Join-Path $work 'payload';New-Item -ItemType Directory -Path $payload|Out-Null
  Copy-Item -LiteralPath (Join-Path $dataDir 'shops.json') -Destination $payload
  if(Test-Path (Join-Path $dataDir 'database-generation.json')){Copy-Item -LiteralPath (Join-Path $dataDir 'database-generation.json') -Destination $payload}
  foreach($dir in @('shops','wb','mercadolibre')){if(Test-Path (Join-Path $dataDir $dir)){Copy-Item -Recurse -LiteralPath (Join-Path $dataDir $dir) -Destination $payload}}
  $zip=Join-Path $work 'snapshot.zip';Compress-Archive -Path (Join-Path $payload '*') -DestinationPath $zip -CompressionLevel Optimal;New-Item -ItemType Directory -Force -Path $deviceDir|Out-Null;Encrypt-File $zip $package (Get-Key)
  $hash=(Get-FileHash -Algorithm SHA256 $package).Hash;@{schemaVersion=1;slot=$slotName;machine=$env:COMPUTERNAME;createdAt=(Get-Date).ToUniversalTime().ToString('o');sha256=$hash;size=(Get-Item $package).Length}|ConvertTo-Json|Set-Content -LiteralPath $manifest -Encoding UTF8
  git -c "safe.directory=$RepositoryRoot" -C $RepositoryRoot pull --rebase origin main;git -c "safe.directory=$RepositoryRoot" -C $RepositoryRoot add -- "cloud-data/devices/$slotName";git -c "safe.directory=$RepositoryRoot" -C $RepositoryRoot commit -m "data($slotName): update encrypted database snapshot";git -c "safe.directory=$RepositoryRoot" -C $RepositoryRoot push origin main
 } else {
  git -c "safe.directory=$RepositoryRoot" -C $RepositoryRoot pull --ff-only origin main;if(!(Test-Path $package)){throw "云端没有数据库 $slotName 快照"};$m=Get-Content -Raw $manifest|ConvertFrom-Json;$actual=(Get-FileHash -Algorithm SHA256 $package).Hash;if($actual-ne$m.sha256){throw '云端数据包 SHA-256 校验失败'}
  $zip=Join-Path $work 'snapshot.zip';Decrypt-File $package $zip (Get-Key);$payload=Join-Path $work 'payload';Expand-Archive -LiteralPath $zip -DestinationPath $payload
  $backup=Join-Path $configDir ("backups\$slotName\"+(Get-Date -Format 'yyyyMMdd-HHmmss'));New-Item -ItemType Directory -Force -Path $backup|Out-Null;if(Test-Path $dataDir){Copy-Item -Recurse -LiteralPath $dataDir -Destination $backup}
  foreach($name in @('shops.json','database-generation.json','shops','wb','mercadolibre')){$src=Join-Path $payload $name;if(Test-Path $src){$dst=Join-Path $dataDir $name;if(Test-Path $dst){Remove-Item -Recurse -Force -LiteralPath $dst};Copy-Item -Recurse -LiteralPath $src -Destination $dst}}
  Write-Output "数据库 $slotName 已恢复；原数据备份：$backup"
 }
}finally{if(Test-Path $work){Remove-Item -Recurse -Force -LiteralPath $work}}
