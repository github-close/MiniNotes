# MiniNotes 发版脚本：版本号 bump → 构建 → 提交/打 tag → 推送 → 打包 zip → GitHub Release。
# 用法（默认 dry-run，只打印计划，不落任何动作）：
#   .\release.ps1                     # dry-run，patch 档
#   .\release.ps1 -Bump minor         # dry-run，minor 档
#   .\release.ps1 -Confirm            # 按 dry-run 展示的计划真实发布
#   .\release.ps1 -Confirm -Bump minor -NotesFile .\notes.md
# 前置：工作区干净、在 main 分支、目标 tag 不存在、MiniNotes 未在运行（exe 被占用无法构建）。
# 发布说明为增量式：只写相对上个 tag 的变化（-NotesFile 指定正文，或用自动生成的提交草稿）。
param(
    [ValidateSet("patch", "minor", "major")]
    [string]$Bump = "patch",
    [string]$Version = "",     # 显式指定版本号（x.y.z），优先于 -Bump
    [string]$NotesFile = "",   # 增量式发布说明正文；不传则自动用「上个 tag 以来的提交列表」做草稿
    [switch]$Confirm           # 不加 = dry-run；加了才真正 commit/push/发 Release
)

$ErrorActionPreference = "Stop"
$repo = $PSScriptRoot
Set-Location $repo
# 原生命令输出（git log 的中文提交说明）在 PS 5.1 默认按控制台 OEM 编码（GBK）
# 解码，捕获进发布说明草稿会整体乱码——显式声明按 UTF-8 解码 git 的输出
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$ExePath = Join-Path $repo "target\release\mininotes.exe"

function Step($n, $msg) { Write-Host ("[{0}] {1}" -f $n, $msg) -ForegroundColor Cyan }
function Die($msg) { Write-Host "✗ $msg" -ForegroundColor Red; exit 1 }
# 封装 git：非零退出即中止，避免半截状态
function Git([string[]]$gitArgs) {
    git.exe @gitArgs
    if ($LASTEXITCODE -ne 0) { Die ("git " + ($gitArgs -join ' ') + " 失败") }
}

# ---------- 预检 ----------
# 发布完整性只关乎已跟踪文件（构建输入全部来自 git 跟踪的源码）：已跟踪文件
# 必须干净；未跟踪文件不进入构建，仅提示不拦截
if (git.exe status --porcelain --untracked-files=no) { Die "有未提交的改动，请先提交或撤销" }
$untracked = git.exe status --porcelain
if ($untracked) { Write-Host "提示：存在未跟踪文件（不影响发布）：`n$untracked" -ForegroundColor DarkGray }
if ((git.exe rev-parse --abbrev-ref HEAD) -ne "main") { Die "发版必须在 main 分支上" }
$running = Get-Process mininotes -ErrorAction SilentlyContinue
if ($running) { Die "MiniNotes 正在运行（PID $($running.Id)），exe 被占用无法构建。请先退出（托盘右键 → 退出）再发版" }

# 计算新版本号：-Version 显式指定，否则按档位 bump
if ($Version -ne "") {
    if ($Version -notmatch '^\d+\.\d+\.\d+$') { Die "版本号格式应为 x.y.z：$Version" }
    $newVer = $Version
} else {
    $m = [regex]::Match((Get-Content Cargo.toml -Raw), '(?m)^version = "(\d+)\.(\d+)\.(\d+)"')
    if (-not $m.Success) { Die "Cargo.toml 里找不到 version 行" }
    $maj = [int]$m.Groups[1].Value; $min = [int]$m.Groups[2].Value; $pat = [int]$m.Groups[3].Value
    switch ($Bump) {
        "major" { $maj++; $min = 0; $pat = 0 }
        "minor" { $min++; $pat = 0 }
        "patch" { $pat++ }
    }
    $newVer = "$maj.$min.$pat"
}
$tag = "v$newVer"
if (git.exe tag -l $tag) { Die "本地已存在 tag $tag" }
if (git.exe ls-remote origin "refs/tags/$tag") { Die "远端已存在 tag $tag" }

# 发布说明（增量式）：-NotesFile 优先，否则自动取上个 tag 以来的提交列表做草稿
$lastTag = git.exe tag --sort=-creatordate | Select-Object -First 1
if ($NotesFile -ne "") {
    $body = [IO.File]::ReadAllText($NotesFile)
} elseif ($lastTag) {
    $log = git.exe log "$lastTag..HEAD" --pretty=format:"- %h %s"
    $body = "$newVer 变更草稿（发版前请把提交列表改写成正式说明）：`n`n" + ($log -join "`n")
} else {
    $body = "首个公开版本。"
}
# 固定追加「下载」节（口径与 README 一致）
$body += @"

## 下载
下载附件 mininotes-$newVer-x64.zip，**解压到任意目录后运行** mininotes.exe（不要在压缩包里直接双击）。压缩包不含数据文件，数据目录首次运行后自动生成：exe 同目录存在 data\ 文件夹 → 便携模式（拷走目录即备份）；否则 → %APPDATA%\MiniNotes\data。详见 README。
"@

# ---------- 计划 ----------
Write-Host ""
Write-Host "========== 发版计划（v$newVer，$Bump）=========" -ForegroundColor Yellow
Write-Host "1. Cargo.toml 版本号 → $newVer；cargo build --release"
Write-Host "2. 提交 chore: v$newVer（Cargo.toml + Cargo.lock）"
Write-Host "3. 打附注 tag：$tag"
Write-Host "4. 推送 main 与 $tag"
Write-Host "5. 打包 mininotes-$newVer-x64.zip（exe + README + LICENSE）"
Write-Host "6. 创建 GitHub Release 并上传 zip"
Write-Host ""
Write-Host "发布说明草稿（创建后可在 GitHub Release 页编辑）："
Write-Host $body -ForegroundColor Gray
Write-Host "=========================================" -ForegroundColor Yellow

if (-not $Confirm) {
    Write-Host "`n以上是 dry-run 预览，未做任何改动。确认无误后加 -Confirm 真实发布。" -ForegroundColor Yellow
    exit 0
}

# ---------- 执行 ----------
Step 1 "版本号 → $newVer，构建 release"
$cargo = Get-Content Cargo.toml -Raw
$cargo = $cargo -replace '(?m)^version = "\d+\.\d+\.\d+"', ("version = `"$newVer`"")
[IO.File]::WriteAllText("$repo\Cargo.toml", $cargo, (New-Object Text.UTF8Encoding $false))
cargo build --release
if ($LASTEXITCODE -ne 0) { Die "cargo build --release 失败" }

Step 2 "提交 chore: v$newVer"
Git @("add", "Cargo.toml", "Cargo.lock")
Git @("commit", "-m", "chore: v$newVer")

Step 3 "打附注 tag $tag"
Git @("tag", "-a", $tag, "-m", "MiniNotes $tag")

Step 4 "推送 main 与 $tag"
Git @("push", "origin", "main")
Git @("push", "origin", $tag)

Step 5 "打包 mininotes-$newVer-x64.zip"
$stage = Join-Path $env:TEMP "mininotes-release-$newVer"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item $ExePath, (Join-Path $repo "README.md"), (Join-Path $repo "LICENSE") $stage
$zip = Join-Path $env:TEMP "mininotes-$newVer-x64.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path "$stage\*" -DestinationPath $zip -Force

Step 6 "创建 GitHub Release 并上传附件"
$fill = "protocol=https`nhost=github.com`n" | git.exe credential fill
$token = ($fill | Where-Object { $_ -like 'password=*' }) -replace '^password=', ''
if (-not $token) { Die "未能从凭据管理器取得 GitHub 令牌" }

$payload = @{ tag_name = $tag; name = "MiniNotes $newVer"; body = $body; draft = $false; prerelease = $false } | ConvertTo-Json
$payloadFile = Join-Path $env:TEMP "release-$newVer-payload.json"
[IO.File]::WriteAllText($payloadFile, $payload, (New-Object Text.UTF8Encoding $false))

$code = curl.exe -s -o "$env:TEMP\release-resp.json" -w "%{http_code}" -X POST `
    -H "Authorization: token $token" -H "Accept: application/vnd.github+json" `
    -d "@$payloadFile" "https://api.github.com/repos/github-close/MiniNotes/releases"
if ($code -ne "201") { Get-Content "$env:TEMP\release-resp.json" -Encoding UTF8; Die "创建 Release 失败：HTTP $code" }
$rid = (Get-Content "$env:TEMP\release-resp.json" -Encoding UTF8 -Raw | ConvertFrom-Json).id

$code = curl.exe -s -o "$env:TEMP\asset-resp.json" -w "%{http_code}" -X POST `
    -H "Authorization: token $token" -H "Content-Type: application/zip" `
    --data-binary "@$zip" "https://uploads.github.com/repos/github-close/MiniNotes/releases/$rid/assets?name=mininotes-$newVer-x64.zip"
if ($code -ne "201") { Get-Content "$env:TEMP\asset-resp.json" -Encoding UTF8; Die "上传附件失败：HTTP $code" }

Remove-Item $stage, $zip, $payloadFile, "$env:TEMP\release-resp.json", "$env:TEMP\asset-resp.json" -Recurse -Force -ErrorAction SilentlyContinue
Write-Host ""
Write-Host "✓ v$newVer 发布完成" -ForegroundColor Green
Write-Host "  https://github.com/github-close/MiniNotes/releases/tag/$tag"
Write-Host "  发布说明可在该页面直接编辑。"
