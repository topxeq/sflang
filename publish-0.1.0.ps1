# ============================================================
# sf (Sflang) 发布到仙缘渡 (magicdo.top) — v0.1.0 (Windows x64 + Linux x64)
# 用法：
#   .\publish-0.1.0.ps1            # 正式发布
#   .\publish-0.1.0.ps1 -DryRun    # 只检查不写
#
# 参考: D:\aiprjs\xxssh\publish-0.5.2.ps1 的成熟模式
# ============================================================
param([switch]$DryRun)

$ErrorActionPreference = "Stop"
$BASE = "https://magicdo.top"
$PRODUCT_ID = "sf"
$VERSION = "0.1.0"
$TMP = $env:TEMP
$UTF8 = [System.Text.UTF8Encoding]::new($false)
$CURL = "C:\Windows\System32\curl.exe"
$CONF = "D:\aiprjs\sflang\repo\sf.conf.json"
$ICON = "D:\aiprjs\sflang\repo\sf-icon.png"

# ---- 平台 → 二进制文件 映射 ----
$PLATFORMS = @(
    (New-Object PSObject -Property @{name="Windows";     file="D:\aiprjs\sflang\repo\target\release\sf.exe"; filename="sf.exe";                 platform="Windows";     arch="x64"}),
    (New-Object PSObject -Property @{name="Linux x64";   file="D:\aiprjs\sflang\result\sf-linux-amd64";      filename="sf-linux-amd64.bin";     platform="Linux x64";   arch="amd64"}),
    (New-Object PSObject -Property @{name="Linux ARM64"; file="D:\aiprjs\sflang\result\sf-arm64";            filename="sf-linux-arm64.bin";     platform="Linux ARM64"; arch="arm64"}),
    (New-Object PSObject -Property @{name="macOS";       file="D:\aiprjs\sflang\result\sf-universal";        filename="sf-macos-universal.bin"; platform="macOS";       arch="universal"})
)

Write-Host ""
Write-Host "==================================================" -ForegroundColor Cyan
Write-Host "  sf v$VERSION  ->  magicdo.top" -ForegroundColor Cyan
Write-Host "==================================================" -ForegroundColor Cyan
if ($DryRun) { Write-Host "  [DRY-RUN] 只检查不写" -ForegroundColor Yellow }
Write-Host ""

# ============================================================
# 1. 发布前检查：文件存在 + 大小限制 + JSON 有效性
# ============================================================
Write-Host "=== 1. 发布前检查 ===" -ForegroundColor Cyan

if (-not (Test-Path $CONF)) { Write-Host "FATAL: 配置不存在 $CONF" -ForegroundColor Red; exit 1 }
try { $cfg = Get-Content $CONF -Raw -Encoding UTF8 | ConvertFrom-Json } catch { Write-Host "FATAL: 配置 JSON 无效: $_" -ForegroundColor Red; exit 1 }
Write-Host "  [OK] 配置: $($cfg.name) / $($cfg.subtitle)" -ForegroundColor Green

if (-not (Test-Path $ICON)) { Write-Host "FATAL: 图标不存在 $ICON" -ForegroundColor Red; exit 1 }
$iconKB = [math]::Round((Get-Item $ICON).Length / 1024.0, 1)
if ($iconKB -gt 500) { Write-Host "FATAL: 图标 ${iconKB}KB 超过 500KB 限制" -ForegroundColor Red; exit 1 }
Write-Host "  [OK] 图标: sf-icon.png (${iconKB}KB)" -ForegroundColor Green

foreach ($p in $PLATFORMS) {
    if (-not (Test-Path $p.file)) { Write-Host "FATAL: $($p.name) 二进制不存在: $($p.file)" -ForegroundColor Red; exit 1 }
    $sizeMB = [math]::Round((Get-Item $p.file).Length / (1048576.0), 2)
    if ($sizeMB -gt 100) { Write-Host "FATAL: $($p.name) ${sizeMB}MB 超过 100MB 限制" -ForegroundColor Red; exit 1 }
    $hash = (Get-FileHash $p.file -Algorithm SHA256).Hash.ToLower()
    Write-Host ("  [OK] {0,-11} {1,7:N2} MB  sha256={2}" -f $p.name, $sizeMB, $hash.Substring(0,16)) -ForegroundColor Green
}

if ($DryRun) {
    Write-Host ""
    Write-Host "=== Dry-run 完成，所有检查通过（未做任何修改）===" -ForegroundColor Green
    exit 0
}

# ============================================================
# 2. 登录（凭据从环境变量读取：MAGICDO_USERNAME 缺省 topget，MAGICDO_PASSWORD 必填）
# ============================================================
Write-Host ""
Write-Host "=== 2. 登录 ===" -ForegroundColor Cyan
if (-not $env:MAGICDO_PASSWORD) { Write-Host "FATAL: 未设置环境变量 MAGICDO_PASSWORD（magicdo.top 管理密码）" -ForegroundColor Red; exit 1 }
$magicdoUser = if ($env:MAGICDO_USERNAME) { $env:MAGICDO_USERNAME } else { "topget" }
$loginJson = '{"username":"' + $magicdoUser + '","password":"' + $env:MAGICDO_PASSWORD + '"}'
[System.IO.File]::WriteAllText("$TMP\sf_login.json", $loginJson, $UTF8)
& $CURL -sS --max-time 30 -X POST "$BASE/api/admin/auth" -H "Content-Type: application/json" --data-binary "@$TMP\sf_login.json" -o "$TMP\sf_login_resp.json"
$loginResp = Get-Content "$TMP\sf_login_resp.json" -Raw -Encoding UTF8 | ConvertFrom-Json
if (-not $loginResp.success) { Write-Host "FATAL: 登录失败: $($loginResp.message)" -ForegroundColor Red; exit 1 }
$TOKEN = $loginResp.token
Write-Host "  [OK] token 24h 有效" -ForegroundColor Green

# ============================================================
# 3. 创建 / 更新产品元数据
# ============================================================
Write-Host ""
Write-Host "=== 3. 产品元数据 ===" -ForegroundColor Cyan

& $CURL -sS --max-time 30 "$BASE/api/admin/products?token=$TOKEN&id=$PRODUCT_ID" -o "$TMP\sf_pre.json"
$preRaw = Get-Content "$TMP\sf_pre.json" -Raw -Encoding UTF8
$productExists = ($preRaw -notmatch '"error"') -and ($preRaw -match ('"id":"' + $PRODUCT_ID + '"'))

$productBody = @{
    id          = $PRODUCT_ID
    name        = $cfg.name
    subtitle    = $cfg.subtitle
    category    = $cfg.category
    tags        = $cfg.tags
    description = $cfg.description
    descriptionFormat = "markdown"
    installScripts = $cfg.installScripts
    featured    = [bool]$cfg.featured
    featuredOrder = [int]$cfg.featuredOrder
    homepageBadge = $cfg.homepageBadge
    homepageSection = $cfg.homepageSection
}
$bodyJson = $productBody | ConvertTo-Json -Depth 8 -Compress
[System.IO.File]::WriteAllText("$TMP\sf_prod.json", $bodyJson, $UTF8)

if ($productExists) {
    Write-Host "  产品已存在，PUT 更新..." -ForegroundColor Yellow
    & $CURL -sS --max-time 60 -X PUT "$BASE/api/admin/products?token=$TOKEN" -H "Content-Type: application/json" --data-binary "@$TMP\sf_prod.json" -o "$TMP\sf_prod_resp.json"
} else {
    Write-Host "  新产品，POST 创建..." -ForegroundColor Yellow
    & $CURL -sS --max-time 60 -X POST "$BASE/api/admin/products?token=$TOKEN" -H "Content-Type: application/json" --data-binary "@$TMP\sf_prod.json" -o "$TMP\sf_prod_resp.json"
}
$prodRespRaw = Get-Content "$TMP\sf_prod_resp.json" -Raw -Encoding UTF8
try { $prodResp = $prodRespRaw | ConvertFrom-Json } catch {
    Write-Host "FATAL: 产品创建/更新返回非 JSON: $($prodRespRaw.Substring(0,[math]::Min(200,$prodRespRaw.Length)))" -ForegroundColor Red; exit 1
}
if (-not $prodResp.success) { Write-Host "FATAL: 产品元数据失败: $($prodResp.message)" -ForegroundColor Red; exit 1 }
Write-Host "  [OK] 产品元数据已保存" -ForegroundColor Green

# ============================================================
# 4. 上传图标
# ============================================================
Write-Host ""
Write-Host "=== 4. 上传图标 ===" -ForegroundColor Cyan
& $CURL -sS --max-time 120 -X POST "$BASE/api/admin/upload?token=$TOKEN&productId=$PRODUCT_ID&type=icon&filename=sf-icon.png" -H "Content-Type: image/png" --data-binary "@$ICON" -o "$TMP\sf_icon_resp.json"
$iconRespRaw = Get-Content "$TMP\sf_icon_resp.json" -Raw -Encoding UTF8
try { $iconResp = $iconRespRaw | ConvertFrom-Json } catch {}
if ($iconResp -and $iconResp.success) {
    $iconUrl = $iconResp.url
    Write-Host "  [OK] $iconUrl" -ForegroundColor Green
    $setIconBody = @{ icon = $iconUrl } | ConvertTo-Json -Compress
    [System.IO.File]::WriteAllText("$TMP\sf_seticon.json", $setIconBody, $UTF8)
    & $CURL -sS -X POST "$BASE/api/admin/products?token=$TOKEN&action=setIcon&id=$PRODUCT_ID" -H "Content-Type: application/json" --data-binary "@$TMP\sf_seticon.json" -o "$TMP\sf_seticon_resp.json" | Out-Null
} else {
    Write-Host "  [WARN] 图标上传失败: $iconRespRaw" -ForegroundColor Yellow
}

# ============================================================
# 5. 上传文档
# ============================================================
Write-Host ""
Write-Host "=== 5. 上传文档 ===" -ForegroundColor Cyan
foreach ($doc in $cfg.docs) {
    & $CURL -sS --max-time 30 "$BASE/api/admin/products?token=$TOKEN&id=$PRODUCT_ID" -o "$TMP\sf_docpre.json"
    $docPreRaw = Get-Content "$TMP\sf_docpre.json" -Raw -Encoding UTF8
    $docId = ""
    if ($docPreRaw -match '"id":"(doc-[^"]+)"[^}]*?"slug":"' + [regex]::Escape($doc.slug) + '"') {
        $docId = $Matches[1]
    }
    $docBody = @{
        title = $doc.title; slug = $doc.slug; content = $doc.content
        format = $doc.format; order = [int]$doc.order
    } | ConvertTo-Json -Depth 4 -Compress
    [System.IO.File]::WriteAllText("$TMP\sf_doc.json", $docBody, $UTF8)
    if ($docId -ne "") {
        & $CURL -sS --max-time 60 -X POST "$BASE/api/admin/products?token=$TOKEN&action=updateDoc&id=$PRODUCT_ID&did=$docId" -H "Content-Type: application/json" --data-binary "@$TMP\sf_doc.json" -o "$TMP\sf_doc_resp.json" | Out-Null
        Write-Host "  [OK] 更新文档: $($doc.title) (did=$docId)" -ForegroundColor Green
    } else {
        & $CURL -sS --max-time 60 -X POST "$BASE/api/admin/products?token=$TOKEN&action=addDoc&id=$PRODUCT_ID" -H "Content-Type: application/json" --data-binary "@$TMP\sf_doc.json" -o "$TMP\sf_doc_resp.json" | Out-Null
        $dRespRaw = Get-Content "$TMP\sf_doc_resp.json" -Raw -Encoding UTF8
        try { $dResp = $dRespRaw | ConvertFrom-Json } catch { $dResp = $null }
        if ($dResp -and $dResp.success) {
            Write-Host "  [OK] 新增文档: $($doc.title)" -ForegroundColor Green
        } else {
            Write-Host "  [WARN] 文档失败 ($($doc.title)): $dRespRaw" -ForegroundColor Yellow
        }
    }
}

# ============================================================
# 6. 上传各平台版本
# ============================================================
Write-Host ""
Write-Host "=== 6. 上传各平台版本 ===" -ForegroundColor Cyan

$existingVids = @{}
foreach ($m in [regex]::Matches($preRaw, '"id":"(ver-[^"]+)"[^}]*?"platform":"([^"]+)"[^}]*?"version":"' + [regex]::Escape($VERSION) + '"')) {
    $existingVids[$m.Groups[2].Value] = $m.Groups[1].Value
}

foreach ($p in $PLATFORMS) {
    Write-Host ""
    Write-Host "  --- $($p.name) ---" -ForegroundColor Cyan

    $hash = (Get-FileHash $p.file -Algorithm SHA256).Hash.ToLower()
    $sizeMB = [math]::Round((Get-Item $p.file).Length / (1048576.0), 2)

    $vid = ""
    if ($existingVids.ContainsKey($p.platform)) {
        $vid = $existingVids[$p.platform]
        Write-Host "    复用已有 vid=$vid" -ForegroundColor Gray
    } else {
        $addBody = @{
            version = $VERSION; platform = $p.platform; arch = $p.arch
            filename = $p.filename; sha256 = $hash; isLatest = $true
            minOsVersion = ($cfg.platforms | Where-Object { $_.name -eq $p.name }).minOsVersion
        } | ConvertTo-Json -Depth 3 -Compress
        [System.IO.File]::WriteAllText("$TMP\sf_addver.json", $addBody, $UTF8)
        & $CURL -sS --max-time 120 -X POST "$BASE/api/admin/products?token=$TOKEN&action=addVersion&id=$PRODUCT_ID" -H "Content-Type: application/json" --data-binary "@$TMP\sf_addver.json" -o "$TMP\sf_addver_resp.json"
        $addRaw = Get-Content "$TMP\sf_addver_resp.json" -Raw -Encoding UTF8
        try {
            $addResp = $addRaw | ConvertFrom-Json
            if ($addResp.success) { $vid = $addResp.data.id } else { Write-Host "    addVersion 失败: $($addResp.message)" -ForegroundColor Red; continue }
        } catch { Write-Host "    addVersion 非 JSON: $($addRaw.Substring(0,[math]::Min(120,$addRaw.Length)))" -ForegroundColor Red; continue }
        Write-Host "    vid=$vid" -ForegroundColor Gray
    }

    $uploaded = $false
    for ($i = 1; $i -le 5; $i++) {
        Write-Host "    上传尝试 $i (${sizeMB} MB)..." -ForegroundColor Cyan
        & $CURL -sS --connect-timeout 60 --max-time 900 -X POST "$BASE/api/admin/upload?token=$TOKEN&productId=$PRODUCT_ID&type=package&filename=$($p.filename)&versionId=$vid" -H "Content-Type: application/octet-stream" --data-binary "@$($p.file)" -o "$TMP\sf_up_resp.json"
        $upRaw = Get-Content "$TMP\sf_up_resp.json" -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
        if ($upRaw -match '"success":true') { Write-Host "    [OK] 已上传" -ForegroundColor Green; $uploaded = $true; break }
        $snippet = if ($upRaw) { $upRaw.Substring(0,[math]::Min(150,$upRaw.Length)) } else { "(empty)" }
        Write-Host "    失败: $snippet" -ForegroundColor Yellow
        Start-Sleep -Seconds 5
    }
    if (-not $uploaded) { Write-Host "    [FAIL] 5 次重试仍失败，跳过 $($p.name)" -ForegroundColor Red; continue }

    $updBody = @{ sha256 = $hash; isLatest = $true; arch = $p.arch } | ConvertTo-Json -Depth 3 -Compress
    [System.IO.File]::WriteAllText("$TMP\sf_updver.json", $updBody, $UTF8)
    & $CURL -sS --max-time 60 -X POST "$BASE/api/admin/products?token=$TOKEN&action=updateVersion&id=$PRODUCT_ID&vid=$vid" -H "Content-Type: application/json" --data-binary "@$TMP\sf_updver.json" -o "$TMP\sf_updver_resp.json" | Out-Null
    Write-Host "    [OK] sha256 + isLatest 已更新" -ForegroundColor Green
}

# ============================================================
# 7. 推送到首页（如配置了 featured）
# ============================================================
if ([bool]$cfg.featured) {
    Write-Host ""
    Write-Host "=== 7. 推送到首页 ===" -ForegroundColor Cyan
    $featBody = @{ featured = $true; featuredOrder = [int]$cfg.featuredOrder } | ConvertTo-Json -Compress
    [System.IO.File]::WriteAllText("$TMP\sf_feat.json", $featBody, $UTF8)
    & $CURL -sS --max-time 30 -X POST "$BASE/api/admin/products?token=$TOKEN&action=featured&id=$PRODUCT_ID" -H "Content-Type: application/json" --data-binary "@$TMP\sf_feat.json" -o "$TMP\sf_feat_resp.json" | Out-Null
    Write-Host "  [OK] featured=true, order=$($cfg.featuredOrder)" -ForegroundColor Green
}

# ============================================================
# 7.5 清理旧版本 isLatest（首版无旧版本，防御性执行）
# ============================================================
Write-Host ""
Write-Host "【清理旧版本 isLatest】" -ForegroundColor Cyan
python D:\aiprjs\sflang\repo\cleanup-latest.py $VERSION
if ($LASTEXITCODE -ne 0) { Write-Host "  [WARN] cleanup-latest.py 失败（手动核对 isLatest）" -ForegroundColor Yellow }

# ============================================================
# 8. 验证
# ============================================================
Write-Host ""
Write-Host "=== 8. 验证 ===" -ForegroundColor Cyan
& $CURL -sS --max-time 30 "$BASE/api/products?id=$PRODUCT_ID" -o "$TMP\sf_pub.json"
$pubRaw = Get-Content "$TMP\sf_pub.json" -Raw -Encoding UTF8
$pubMatches = [regex]::Matches($pubRaw, '"isLatest":(true|false)[^}]*?"platform":"([^"]+)"[^}]*?"version":"([^"]+)"')
Write-Host "  最新版本:" -ForegroundColor Gray
foreach ($m in $pubMatches) {
    if ($m.Groups[1].Value -eq "true") {
        Write-Host "    $($m.Groups[2].Value) v$($m.Groups[3].Value)" -ForegroundColor Green
    }
}
& $CURL -sS --max-time 15 "$BASE/install/sf.sh" -o "$TMP\sf_install.sh"
$installHead = (Get-Content "$TMP\sf_install.sh" -Raw -Encoding UTF8).Split("`n")[0..2] -join " | "
Write-Host "  安装端点 .sh: $installHead" -ForegroundColor Gray

Write-Host ""
Write-Host "==================================================" -ForegroundColor Green
Write-Host "  sf v$VERSION 已发布" -ForegroundColor Green
Write-Host "  https://magicdo.top/product.xxl?id=$PRODUCT_ID" -ForegroundColor Green
Write-Host "==================================================" -ForegroundColor Green
