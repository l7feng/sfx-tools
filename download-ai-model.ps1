param([string]$ModelRoot="$PSScriptRoot/build/models/multilingual-e5-small")
$ErrorActionPreference='Stop'
$revision='614241f622f53c4eeff9890bdc4f31cfecc418b3'
$base="https://huggingface.co/intfloat/multilingual-e5-small"
$proxy='http://127.0.0.1:7897'
New-Item -ItemType Directory -Force -Path "$ModelRoot/onnx" | Out-Null
$tree=Invoke-RestMethod -Uri "$base/../../api/models/intfloat/multilingual-e5-small/tree/$revision/onnx" -Proxy $proxy -TimeoutSec 30
$model=$tree | Where-Object {$_.path -eq 'onnx/model.onnx'}
if(!$model -or !$model.lfs.oid){throw '无法取得官方模型大小与SHA256，停止下载'}
$files=@(
 @{remote='onnx/model.onnx';local='onnx/model.onnx';sha=$model.lfs.oid;size=$model.size},
 @{remote='onnx/tokenizer.json';local='tokenizer.json'},
 @{remote='config.json';local='config.json'},
 @{remote='README.md';local='README.md'}
)
$records=@()
foreach($item in $files){
 $target=Join-Path $ModelRoot $item.local
 $valid=(Test-Path -LiteralPath $target) -and (!$item.sha -or (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant() -eq $item.sha)
 if(!$valid){
  Write-Output "下载 $($item.remote)"
  & curl.exe --proxy $proxy --fail --location --retry 2 --retry-all-errors --connect-timeout 20 --max-time 600 --output "$target.part" "$base/resolve/$revision/$($item.remote)"
  if($LASTEXITCODE -ne 0){throw "下载失败，保留part：$target"}
  $hash=(Get-FileHash -LiteralPath "$target.part" -Algorithm SHA256).Hash.ToLowerInvariant()
  if($item.sha -and $hash -ne $item.sha){throw '模型SHA256与官方LFS值不符'}
  Move-Item -LiteralPath "$target.part" -Destination $target -Force
 }
 $records+=@{path=$item.local;sha256=(Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant();size=(Get-Item -LiteralPath $target).Length}
}
@{model_id='intfloat/multilingual-e5-small';revision=$revision;license='MIT';files=$records} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$ModelRoot/model-manifest.json" -Encoding utf8
Write-Output '模型下载与校验完成'
