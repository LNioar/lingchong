<#
.SYNOPSIS
    DeskPet「主动播报」诊断脚本（只读，不修改数据库与配置）

.DESCRIPTION
    在安装了 DeskPet MSI 的 Windows 机器上运行。脚本会自动定位
      %APPDATA%\deskpet\deskpet.db / config.toml / deskpet.log
    （找不到时按 Tauri 标识回退到 %APPDATA%\com.deskpet.app 等目录），
    然后按「进程 → 打扰策略 → 采集 → 候选 → 打分阈值」逐层排查
    "为什么没有主动弹气泡"。

    诊断依据与源码一致：
      · 自动循环只在 can_announce() 为真时运行（勿扰时段 / 静默 / 最小间隔）
      · 候选 = first_seen 在窗口内、未被播报、3 天内未被 dislike
      · score = 0.6*sim + 0.2*heat + 0.15*fresh + 0.05*penalty
      · 无画像用冷启动阈值 0.3；有兴趣画像用 announce_threshold（默认 0.6）

    为保证只读且不受运行中进程的数据库锁影响，脚本会先把
    deskpet.db(+wal/shm) 复制到临时目录再查询，原库不会被写入。

.PARAMETER DataDir
    数据目录。默认自动探测 %APPDATA%\deskpet。

.PARAMETER Hours
    统计窗口（小时），默认 24，与 Agent 回看窗口一致。

.PARAMETER KeepSnapshot
    保留临时数据库快照，便于人工用 sqlite 工具继续分析。

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\diagnose-announce.ps1

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\diagnose-announce.ps1 -Hours 48 -KeepSnapshot
#>
[CmdletBinding()]
param(
    [string]$DataDir = '',
    [int]$Hours = 24,
    [switch]$KeepSnapshot
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# 输出辅助
# ---------------------------------------------------------------------------
function Write-Head([string]$Text) {
    Write-Host ''
    Write-Host ('=' * 62) -ForegroundColor DarkGray
    Write-Host " $Text" -ForegroundColor Cyan
    Write-Host ('=' * 62) -ForegroundColor DarkGray
}
function Write-Kv([string]$Key, $Value) {
    Write-Host ('  {0,-26} {1}' -f $Key, $Value)
}
function Format-Time($Ts) {
    if ($null -eq $Ts -or "$Ts" -eq '' -or [double]$Ts -le 0) { return '-' }
    try {
        return ([DateTimeOffset]::FromUnixTimeSeconds([int64]$Ts)).ToLocalTime().ToString('yyyy-MM-dd HH:mm:ss')
    } catch { return "$Ts" }
}

# ---------------------------------------------------------------------------
# 定位数据目录
# ---------------------------------------------------------------------------
if ($DataDir -eq '') {
    $candidates = @(
        (Join-Path $env:APPDATA 'deskpet'),
        (Join-Path $env:APPDATA 'com.deskpet.app'),
        (Join-Path $env:LOCALAPPDATA 'deskpet'),
        (Join-Path $env:LOCALAPPDATA 'com.deskpet.app')
    )
    foreach ($c in $candidates) {
        if (Test-Path (Join-Path $c 'deskpet.db')) { $DataDir = $c; break }
    }
}

Write-Head 'DeskPet 主动播报诊断'
if ($DataDir -eq '' -or -not (Test-Path (Join-Path $DataDir 'deskpet.db'))) {
    Write-Host '  [×] 未找到 deskpet.db。' -ForegroundColor Red
    Write-Host '      已尝试：%APPDATA%\deskpet、%APPDATA%\com.deskpet.app、%LOCALAPPDATA%\deskpet' -ForegroundColor DarkGray
    Write-Host '      请用 -DataDir 指定实际数据目录。' -ForegroundColor DarkGray
    exit 2
}
Write-Kv '数据目录' $DataDir
Write-Kv '统计窗口' "$Hours 小时"
Write-Kv '诊断时间' (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')

# ---------------------------------------------------------------------------
# 选择查询后端：优先 sqlite3.exe，其次 python，最后提示安装
# ---------------------------------------------------------------------------
$script:Sqlite = $null
$script:Python = $null
$script:DbPath = $null

$s = (Get-Command sqlite3 -ErrorAction SilentlyContinue).Source
if ($s) { $script:Sqlite = $s }
if (-not $script:Sqlite) {
    $roots = @(
        (Join-Path $env:LOCALAPPDATA 'Android\Sdk\platform-tools'),
        (Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet\Packages'),
        $env:ProgramFiles,
        ${env:ProgramFiles(x86)}
    )
    foreach ($r in $roots) {
        if ($r -and (Test-Path $r)) {
            $f = Get-ChildItem $r -Filter 'sqlite3.exe' -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($f) { $script:Sqlite = $f.FullName; break }
        }
    }
}
if (-not $script:Sqlite) {
    foreach ($n in @('python', 'python3', 'py')) {
        $p = (Get-Command $n -ErrorAction SilentlyContinue).Source
        if ($p) { $script:Python = $p; break }
    }
}
if (-not $script:Sqlite -and -not $script:Python) {
    Write-Host ''
    Write-Host '  [×] 需要 sqlite3.exe 或 Python 才能读取数据库。任选其一安装：' -ForegroundColor Yellow
    Write-Host '      winget install --id SQLite.SQLite -e' -ForegroundColor White
    Write-Host '      winget install --id Python.Python.3.12 -e' -ForegroundColor White
    exit 3
}
Write-Kv 'SQL 后端' ($(if ($script:Sqlite) { "sqlite3: $script:Sqlite" } else { "python: $script:Python" }))

# ---------------------------------------------------------------------------
# 复制快照（避免与运行中的进程争锁，绝不写原库）
# ---------------------------------------------------------------------------
$snapDir = Join-Path $env:TEMP ('deskpet-diag-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $snapDir -Force | Out-Null
foreach ($name in @('deskpet.db', 'deskpet.db-wal', 'deskpet.db-shm')) {
    $src = Join-Path $DataDir $name
    if (Test-Path $src) { Copy-Item $src (Join-Path $snapDir $name) -Force }
}
$script:DbPath = Join-Path $snapDir 'deskpet.db'
Write-Kv '数据库快照' $snapDir

$pyCode = @'
import sqlite3, json, sys, base64
sql = base64.b64decode(sys.argv[1]).decode('utf-8')
path = sys.argv[2].replace('\\', '/')
con = sqlite3.connect('file:' + path + '?mode=ro', uri=True)
cur = con.cursor()
cur.execute(sql)
cols = [d[0] for d in cur.description] if cur.description else []
rows = [dict(zip(cols, r)) for r in cur.fetchall()]
print(json.dumps(rows, default=str, ensure_ascii=False))
'@

function Invoke-Sql([string]$Sql) {
    if ($script:Sqlite) {
        $out = & $script:Sqlite -readonly -csv -header $script:DbPath $Sql 2>$null
        if (-not $out) { return @() }
        return @($out | ConvertFrom-Csv)
    }
    $b64 = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($Sql))
    $out = & $script:Python -c $pyCode $b64 $script:DbPath 2>$null
    if (-not $out) { return @() }
    return @(($out | Out-String | ConvertFrom-Json))
}

# ---------------------------------------------------------------------------
# 1. 进程
# ---------------------------------------------------------------------------
Write-Head '1. 进程状态'
$procs = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -match 'deskpet' })
if ($procs.Count -gt 0) {
    foreach ($p in $procs) {
        Write-Kv '运行中' ("{0} (PID {1}, 启动 {2})" -f $p.ProcessName, $p.Id, $p.StartTime)
    }
} else {
    Write-Host '  [!] 没有发现 DeskPet 进程：自动播报循环不会运行。' -ForegroundColor Yellow
}

# ---------------------------------------------------------------------------
# 2. 配置
# ---------------------------------------------------------------------------
Write-Head '2. 配置（打扰策略 / 模型）'
$cfgPath = Join-Path $DataDir 'config.toml'
$cfg = [ordered]@{
    quiet_hours     = ''
    min_interval    = 45
    threshold       = 0.6
    api_base        = ''
    api_key_set     = $false
    model           = ''
    embedding_model = ''
    rsshub          = ''
    platforms       = @()
}
if (-not (Test-Path $cfgPath)) {
    Write-Host '  [!] 未找到 config.toml，按代码默认值判断（勿扰 23:00-08:00，间隔 45 分钟，阈值 0.6）' -ForegroundColor Yellow
    $cfg.quiet_hours = '["23:00-08:00"]'
} else {
    $sec = ''
    foreach ($raw in Get-Content $cfgPath) {
        $line = $raw.Trim()
        if ($line -eq '' -or $line.StartsWith('#')) { continue }
        if ($line -match '^\[\[(.+)\]\]$') {
            $sec = $Matches[1]
            if ($sec -eq 'fetch.platforms') { $cfg.platforms += [pscustomobject]@{ id = ''; enabled = $true; interval = 0 } }
            continue
        }
        if ($line -match '^\[(.+)\]$') { $sec = $Matches[1]; continue }
        if ($line -notmatch '^([A-Za-z0-9_\.\-]+)\s*=\s*(.+)$') { continue }
        $k = $Matches[1]
        $v = ($Matches[2] -replace '\s+#.*$', '').Trim()
        switch ("$sec.$k") {
            'general.quiet_hours'                { $cfg.quiet_hours = $v }
            'general.min_announce_interval_min'  { $cfg.min_interval = [int]$v }
            'general.announce_threshold'         { $cfg.threshold = [double]$v }
            'llm.api_base'                       { $cfg.api_base = $v.Trim('"') }
            'llm.api_key'                        { $cfg.api_key_set = ($v.Trim('"').Length -gt 0) }
            'llm.model'                          { $cfg.model = $v.Trim('"') }
            'llm.embedding_model'                { $cfg.embedding_model = $v.Trim('"') }
            'fetch.rsshub_endpoint'              { $cfg.rsshub = $v.Trim('"') }
        }
        if ($sec -eq 'fetch.platforms' -and $cfg.platforms.Count -gt 0) {
            $p = $cfg.platforms[$cfg.platforms.Count - 1]
            if ($k -eq 'id') { $p.id = $v.Trim('"') }
            elseif ($k -eq 'enabled') { $p.enabled = ($v -eq 'true') }
            elseif ($k -eq 'interval_min') { $p.interval = [int]$v }
        }
    }
}

$quietRanges = @()
if ($cfg.quiet_hours) {
    $quietRanges = @([regex]::Matches($cfg.quiet_hours, '"([^"]+)"') | ForEach-Object { $_.Groups[1].Value })
}
$enabledPlatforms = @($cfg.platforms | Where-Object { $_.enabled } | ForEach-Object { $_.id })

Write-Kv '勿扰时段' ($(if ($quietRanges.Count) { $quietRanges -join ', ' } else { '无' }))
Write-Kv '最小播报间隔' ("{0} 分钟" -f $cfg.min_interval)
Write-Kv '自动播报阈值' ("{0:N2}" -f $cfg.threshold)
Write-Kv '启用平台' ($(if ($enabledPlatforms.Count) { $enabledPlatforms -join ', ' } else { '（空）' }))
Write-Kv 'LLM api_base' ($(if ($cfg.api_base) { $cfg.api_base } else { '（空）' }))
Write-Kv 'LLM 模型' ($(if ($cfg.model) { $cfg.model } else { '（空）' }))
Write-Kv 'API Key 已填写' $cfg.api_key_set
Write-Kv 'embedding_model' ($(if ($cfg.embedding_model) { $cfg.embedding_model } else { '（未设置 → 向量功能关闭）' }))

$inQuiet = $false
$now = Get-Date
$curMin = $now.Hour * 60 + $now.Minute
foreach ($r in $quietRanges) {
    if ($r -match '^(\d{1,2}):(\d{2})\s*-\s*(\d{1,2}):(\d{2})$') {
        $qs = [int]$Matches[1] * 60 + [int]$Matches[2]
        $qe = [int]$Matches[3] * 60 + [int]$Matches[4]
        if ($qs -le $qe) { if ($curMin -ge $qs -and $curMin -lt $qe) { $inQuiet = $true } }
        else { if ($curMin -ge $qs -or $curMin -lt $qe) { $inQuiet = $true } }
    }
}
if ($inQuiet) {
    Write-Host '  [!] 当前时间处于勿扰时段，自动播报被策略拦截（手动双击不受影响）。' -ForegroundColor Yellow
} else {
    Write-Host '  [√] 当前不在勿扰时段。' -ForegroundColor Green
}

# ---------------------------------------------------------------------------
# 3. 数据库总览
# ---------------------------------------------------------------------------
Write-Head '3. 数据库总览'
$window = $Hours * 3600

# 先看表结构：旧版安装包可能没有 announcements 表
$tableRows = Invoke-Sql "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
$tableNames = @($tableRows | ForEach-Object { $_.name })
$hasAnnTable = $tableNames -contains 'announcements'
Write-Kv '数据表' ($tableNames -join ', ')
if (-not $hasAnnTable) {
    Write-Host '  [!] 缺少 announcements 表：安装包版本落后于当前代码，或建表失败。' -ForegroundColor Yellow
}

$stats = Invoke-Sql @"
SELECT
  (SELECT COUNT(*) FROM hot_items)                          AS hot_total,
  (SELECT COUNT(*) FROM feedback)                           AS fb_total,
  (SELECT COUNT(*) FROM fetch_logs)                         AS fetch_total,
  (SELECT COALESCE(MAX(started_at), 0) FROM fetch_logs)      AS last_fetch,
  (SELECT COALESCE(MAX(first_seen), 0) FROM hot_items)       AS newest_item
"@
$st = $stats | Select-Object -First 1

# sqlite3 csv 输出全是字符串，统一转数字
function To-Int($v) { if ($null -eq $v -or "$v" -eq '') { return 0 } return [int64]$v }

$hotTotal    = To-Int $st.hot_total
$annTotal    = 0
$lastAnnounce = 0
if ($hasAnnTable) {
    $annRow = Invoke-Sql "SELECT COUNT(*) AS ann_total, COALESCE(MAX(announced_at),0) AS last_announce FROM announcements" | Select-Object -First 1
    $annTotal = To-Int $annRow.ann_total
    $lastAnnounce = To-Int $annRow.last_announce
}
$lastFetch   = To-Int $st.last_fetch
$newestItem  = To-Int $st.newest_item
$nowTs       = [int64]([DateTimeOffset]::UtcNow.ToUnixTimeSeconds())
$windowStart = $nowTs - $window

Write-Kv '热点总数' $hotTotal
Write-Kv '已播报条数' ($(if ($hasAnnTable) { $annTotal } else { '（无 announcements 表）' }))
Write-Kv '最近播报时间' ($(if ($hasAnnTable) { Format-Time $lastAnnounce } else { '-' }))
Write-Kv '最近采集时间' (Format-Time $lastFetch)
Write-Kv '最新热点时间' (Format-Time $newestItem)
if ($lastFetch -gt 0) { Write-Kv '距上次采集' ("{0:N1} 小时" -f (($nowTs - $lastFetch) / 3600.0)) }
if ($lastAnnounce -gt 0) { Write-Kv '距上次播报' ("{0:N1} 小时" -f (($nowTs - $lastAnnounce) / 3600.0)) }

# 每个平台窗口内入库
$perPlatform = Invoke-Sql @"
SELECT platform,
       COUNT(*) AS items,
       MAX(last_seen) AS newest_seen,
       SUM(CASE WHEN tags = '[]' THEN 1 ELSE 0 END) AS no_tags
FROM hot_items
WHERE first_seen >= $windowStart
GROUP BY platform
ORDER BY items DESC
"@
Write-Host ''
Write-Host '  窗口内入库情况：'
if (@($perPlatform).Count -eq 0) {
    Write-Host '    （窗口内没有任何热点入库）' -ForegroundColor Yellow
} else {
    foreach ($row in $perPlatform) {
        Write-Host ('    {0,-14} {1,5} 条   最新 {2}   无标签 {3}' -f $row.platform, $row.items, (Format-Time $row.newest_seen), $row.no_tags)
    }
}

# ---------------------------------------------------------------------------
# 4. 候选漏斗
# ---------------------------------------------------------------------------
Write-Head '4. 候选漏斗（决定"有没有东西可播"）'
$notAnnounced = $(if ($hasAnnTable) { "NOT EXISTS (SELECT 1 FROM announcements a WHERE a.hot_id = h.id)" } else { "1 = 1" })
$funnel = Invoke-Sql @"
SELECT
  (SELECT COUNT(*) FROM hot_items h
     WHERE h.first_seen >= $windowStart
       AND $notAnnounced)                                                       AS unannounced_window,
  (SELECT COUNT(*) FROM hot_items h WHERE $notAnnounced)                        AS unannounced_all,
  (SELECT COUNT(DISTINCT h.id) FROM hot_items h
     JOIN feedback f ON f.hot_id = h.id
     WHERE f.action = 'dislike'
       AND f.at >= $($nowTs - 259200)
       AND $notAnnounced)                                                       AS disliked_unannounced,
  (SELECT COUNT(DISTINCT h.id) FROM hot_items h
     JOIN feedback f ON f.hot_id = h.id
     WHERE f.action IN ('ignore','dismiss')
       AND f.at >= $($nowTs - 259200))                                          AS ignored_recent,
  (SELECT COUNT(*) FROM hot_items h
     WHERE h.first_seen < $windowStart
       AND $notAnnounced)                                                       AS older_unannounced
"@
$fn = $funnel | Select-Object -First 1
$unannouncedWindow = To-Int $fn.unannounced_window
$unannouncedAll    = To-Int $fn.unannounced_all
$dislikedUnann     = To-Int $fn.disliked_unannounced
$ignoredRecent     = To-Int $fn.ignored_recent
$olderUnannounced  = To-Int $fn.older_unannounced

Write-Kv '窗口内未播报候选' $unannouncedWindow
Write-Kv '全库未播报条目' $unannouncedAll
Write-Kv '其中 3 天内被 dislike（硬排除）' $dislikedUnann
Write-Kv '3 天内 ignore/dismiss（软惩罚）' $ignoredRecent
Write-Kv '窗口外未播报（重启前不会被看到）' $olderUnannounced

# 最近播报明细
if ($hasAnnTable) {
    $lastAnn = Invoke-Sql "SELECT datetime(announced_at,'unixepoch','localtime') AS t, substr(hot_id,1,12) AS id FROM announcements ORDER BY announced_at DESC LIMIT 5"
    if (@($lastAnn).Count -gt 0) {
        Write-Host ''
        Write-Host '  最近播报：'
        foreach ($a in $lastAnn) { Write-Host ('    {0}  {1}…' -f $a.t, $a.id) }
    }
}

# ---------------------------------------------------------------------------
# 5. 画像与阈值
# ---------------------------------------------------------------------------
Write-Head '5. 画像与打分阈值（最常见的"永远不播"原因）'
$profileRow = Invoke-Sql "SELECT value, updated_at FROM profile WHERE key = 'profile'" | Select-Object -First 1
$interestCount = 0
$profileUpdated = 0
if ($profileRow) {
    $profileUpdated = To-Int $profileRow.updated_at
    $json = [string]$profileRow.value
    if ($json -match '"interests"\s*:\s*\[(.*?)\]') {
        $interestCount = @([regex]::Matches($Matches[1], '"topic"\s*:')).Count
    }
}
Write-Kv '画像兴趣数' $interestCount
Write-Kv '画像更新时间' (Format-Time $profileUpdated)

$embeddingUsable = ($cfg.embedding_model -ne '') -and ($cfg.api_base -notmatch 'deepseek')
$effThreshold = $(if ($interestCount -eq 0) { 0.3 } else { $cfg.threshold })
Write-Kv '实际生效阈值' ("{0:N2}（{1}）" -f $effThreshold, $(if ($interestCount -eq 0) { '冷启动' } else { '画像已建立' }))
Write-Kv '向量 embedding 可用' $embeddingUsable

# 相似度为 0 时，score 上限 = 0.2*heat + 0.15*fresh + 0.05*penalty
$ceilingWithoutSim = 0.2 + 0.15 + 0.05
$scaledThreshold = [math]::Round($cfg.threshold * $ceilingWithoutSim, 2)
if ($interestCount -gt 0 -and -not $embeddingUsable) {
    Write-Host ''
    Write-Host ('  [!] 画像已建立但 embedding 不可用 → sim 恒为 0，此时 score 上限只有 {0:N2}。' -f $ceilingWithoutSim) -ForegroundColor Yellow
    Write-Host ('      含阈值折算修复的新版本会把 {0:N2} 折算为 {1:N2} 再比较，自动播报仍可触发；' -f $cfg.threshold, $scaledThreshold) -ForegroundColor DarkGray
    Write-Host ('      旧版本仍直接用 {0:N2} 比较，自动播报在数学上不可能发生。' -f $cfg.threshold) -ForegroundColor DarkGray
    Write-Host '      判定依据：日志 [agent] 行出现 sim_available= 即为新版本。' -ForegroundColor DarkGray
}

# ---------------------------------------------------------------------------
# 6. 采集健康度
# ---------------------------------------------------------------------------
Write-Head '6. 采集健康度'
$fetchStats = Invoke-Sql @"
SELECT platform, status, COUNT(*) AS n, SUM(item_count) AS items, MAX(started_at) AS last_at
FROM fetch_logs
WHERE started_at >= $windowStart
GROUP BY platform, status
ORDER BY platform, status
"@
$okCount = 0
$failCount = 0
if (@($fetchStats).Count -eq 0) {
    Write-Host '  窗口内没有采集记录：采集循环可能没跑起来。' -ForegroundColor Yellow
} else {
    foreach ($r in $fetchStats) {
        $n = To-Int $r.n
        if ($r.status -eq 'ok') { $okCount += $n } else { $failCount += $n }
        Write-Host ('    {0,-14} {1,-8} {2,4} 次  入库 {3,4} 条  最近 {4}' -f $r.platform, $r.status, $n, $r.items, (Format-Time $r.last_at))
    }
    $total = $okCount + $failCount
    $rate = if ($total -gt 0) { [math]::Round($okCount * 100.0 / $total, 1) } else { 0 }
    Write-Host ''
    Write-Kv '成功率' ("{0}%  （成功 {1} / 失败 {2}）" -f $rate, $okCount, $failCount)
}

$errs = Invoke-Sql @"
SELECT platform, datetime(started_at,'unixepoch','localtime') AS t, error
FROM fetch_logs
WHERE error IS NOT NULL AND error <> ''
ORDER BY started_at DESC LIMIT 6
"@
if (@($errs).Count -gt 0) {
    Write-Host ''
    Write-Host '  最近采集错误：'
    foreach ($e in $errs) {
        Write-Host ('    [{0}] {1}  {2}' -f $e.platform, $e.t, $e.error) -ForegroundColor DarkYellow
    }
}

# ---------------------------------------------------------------------------
# 7. 日志证据
# ---------------------------------------------------------------------------
Write-Head '7. 日志证据（进程内真实决策）'
$logPath = Join-Path $DataDir 'deskpet.log'
if (-not (Test-Path $logPath)) {
    Write-Host '  未找到 deskpet.log（release 版日志可能未开启或已被清理）。' -ForegroundColor Yellow
} else {
    $lines = @(Get-Content $logPath -Tail 6000 -Encoding UTF8 -ErrorAction SilentlyContinue)
    $counts = [ordered]@{
        '无候选'          = @($lines | Select-String -Pattern '\[agent\] no new candidates').Count
        '低于阈值跳过'    = @($lines | Select-String -Pattern '\[agent\] below threshold').Count
        '采集失败'        = @($lines | Select-String -Pattern 'fetch failed|fallback also failed').Count
        'LLM 失败'        = @($lines | Select-String -Pattern '\[llm\].*(request failed|status=[45]\d\d)').Count
        'LLM 成功'        = @($lines | Select-String -Pattern '\[llm\].*status=200').Count
        'embedding 失败'  = @($lines | Select-String -Pattern 'embedding.*(404|failed)').Count
        '勿扰/静默拦截'   = @($lines | Select-String -Pattern 'quiet|silence').Count
    }
    foreach ($k in $counts.Keys) { Write-Kv $k $counts[$k] }

    $scoreLines = @($lines | Select-String -Pattern '\[agent\] top candidate score=')
    if ($scoreLines.Count -gt 0) {
        Write-Host ''
        Write-Host '  最近一次打分：'
        Write-Host ('    ' + $scoreLines[-1].Line) -ForegroundColor DarkGray
    }
    $llmLines = @($lines | Select-String -Pattern '\[llm\] <<< status=')
    if ($llmLines.Count -gt 0) {
        Write-Host '  最近一次 LLM 响应：'
        Write-Host ('    ' + $llmLines[-1].Line) -ForegroundColor DarkGray
    }
    $logTime = (Get-Item $logPath).LastWriteTime
    Write-Kv '日志最后写入' $logTime
}

# ---------------------------------------------------------------------------
# 8. 结论
# ---------------------------------------------------------------------------
Write-Head '8. 结论与建议'
$issues = New-Object System.Collections.ArrayList

if ($procs.Count -eq 0) {
    [void]$issues.Add('DeskPet 进程未运行 → 定时循环不存在，只能靠手动触发。请确认已启动并且没有被退出。')
}
if ($inQuiet) {
    [void]$issues.Add('当前处于勿扰时段 → can_announce() 返回 false，自动播报被策略拦截。')
}
if ($hotTotal -eq 0) {
    [void]$issues.Add('数据库没有任何热点 → 采集从未成功入库，先修网络/采集源（RSSHub 国内常不可达）。')
}
if (-not $hasAnnTable) {
    [void]$issues.Add('数据库缺少 announcements 表 → 安装的 MSI 版本落后于当前代码（或建表失败）。旧版“是否已播报”的判断在取表失败时会把候选当成已播报，导致永远无候选，请升级/重装最新安装包。')
} elseif ($hotTotal -gt 0 -and $annTotal -eq 0) {
    [void]$issues.Add('热点已入库但 announcements 表为空 → 自动播报一次都没成功过，问题在候选或阈值环节。')
}
if ($interestCount -gt 0 -and -not $embeddingUsable) {
    [void]$issues.Add(("画像已建立（{0} 个兴趣）但 embedding 不可用 → sim=0，score 上限 {1:N2}；新版本折算后阈值为 {2:N2}，旧版本仍按 {3:N2} 比较（必然无播报）。" -f $interestCount, $ceilingWithoutSim, $scaledThreshold, $effThreshold))
}
if ($unannouncedWindow -eq 0 -and $hotTotal -gt 0) {
    [void]$issues.Add(("近 {0} 小时没有未播报候选（可能已全部播报，或窗口内没有新采集）。" -f $Hours))
}
if ($dislikedUnann -gt 0) {
    [void]$issues.Add(("{0} 条未播报热点因 3 天内 dislike 被硬排除。" -f $dislikedUnann))
}
if ($ignoredRecent -gt 0) {
    [void]$issues.Add(("{0} 条近期被 ignore/dismiss，打分惩罚系数 0.3，会显著拉低 score。" -f $ignoredRecent))
}
if ($cfg.embedding_model -ne '' -and $cfg.api_base -match 'deepseek') {
    [void]$issues.Add('embedding_model 已设置但 api_base 指向 DeepSeek——DeepSeek 没有 /embeddings 接口，会稳定 404 并退化为占位向量。')
}
if ($fetchStats -and (($okCount + $failCount) -gt 0)) {
    $rate = $okCount * 100.0 / ($okCount + $failCount)
    if ($rate -lt 60) { [void]$issues.Add(("采集成功率仅 {0:N1}%：rsshub.app 在国内常被阻断，抖音/小红书因此必然失败。" -f $rate)) }
}

if ($issues.Count -eq 0) {
    Write-Host '  未发现明显阻塞项。若仍无播报，请用 -Hours 48 扩大窗口并检查日志中的 [agent] 决策行。' -ForegroundColor Green
} else {
    $i = 1
    foreach ($it in $issues) {
        Write-Host ("  [{0}] {1}" -f $i, $it) -ForegroundColor Yellow
        $i++
    }
}

Write-Host ''
Write-Host '  建议动作：' -ForegroundColor Cyan
Write-Host '    1) 临时验证：双击宠物（手动播报跳过阈值），若能播 → 说明链路正常，问题在阈值/策略。'
Write-Host '    2) 若画像兴趣数 > 0 且没有可用 embedding：升级到含阈值折算修复的版本（日志出现 sim_available=），'
Write-Host '       或配置真正支持 /embeddings 的服务（并把 api_base 指向它）。'
Write-Host '    3) 勿扰时段：确认 general.quiet_hours（默认 23:00-08:00）是否符合预期。'
Write-Host '    4) 采集：rsshub_endpoint 建议改为自建实例；否则关闭 douyin/xiaohongshu 以减少失败噪音。'

# ---------------------------------------------------------------------------
# 清理
# ---------------------------------------------------------------------------
if ($KeepSnapshot) {
    Write-Host ''
    Write-Host ("  快照已保留：{0}" -f $snapDir) -ForegroundColor DarkGray
} else {
    Remove-Item $snapDir -Recurse -Force -ErrorAction SilentlyContinue
}
