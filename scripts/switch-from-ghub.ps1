<#
.SYNOPSIS
  Logicool G HUB から Device HUB へ切り替える (常駐の登録 → G HUB の停止・アンインストール)。

.DESCRIPTION
  既定はドライランで、何も変更せずに「何をするか」だけを表示する。-Apply で実行する。

  手順:
    1. device-hub.exe を %LOCALAPPDATA%\DeviceHub\bin にコピー (自動起動のパスを固定するため)
    2. Windows ログオン時の自動起動を登録 (HKCU の Run キー、--background で最小化起動)
    3. G HUB のプロセス/サービスを停止
    4. G HUB をアンインストール (winget → 見つからなければレジストリのアンインストーラ)
    5. -BackupData を付けると G HUB の設定フォルダをバックアップ先へ移動 (削除はしない)

  やらないこと: G HUB の設定データの削除 (-BackupData でも「移動」のみ)、ドライバの削除。
  マウス本体のオンボードメモリは G HUB を消しても残る。ただし G HUB が PC 側で持っていた
  プロファイル (アプリ別設定・ボタン割り当てのソフト側分) は、アンインストールで使えなくなる。

  ★ 先に確認すること: Device HUB で PRO 2 の DPI とポーリングレートを変え、実機で反映される
    (マウスの挙動が変わる / hidppprobe で値が変わる) ことを確認してから -Apply する。
    ファームウェア更新は G HUB でしか行えない。必要になったら G HUB を再インストールする。

.EXAMPLE
  .\scripts\switch-from-ghub.ps1                       # ドライラン
  .\scripts\switch-from-ghub.ps1 -Apply                # 実行 (確認プロンプトあり)
  .\scripts\switch-from-ghub.ps1 -Apply -BackupData    # 設定フォルダも退避
  .\scripts\switch-from-ghub.ps1 -Apply -SkipUninstall # 自動起動の登録と G HUB の停止だけ
#>
[CmdletBinding()]
param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\target\release\device-hub.exe'),
    [switch]$Apply,
    [switch]$SkipUninstall,
    [switch]$BackupData,
    [switch]$Yes
)

$ErrorActionPreference = 'Stop'

function Step($msg) { Write-Host "`n==> $msg" -ForegroundColor Cyan }
function Do-Or-Say($desc, [scriptblock]$action) {
    if ($Apply) { & $action } else { Write-Host "  [dry-run] $desc" -ForegroundColor DarkGray }
}

if (-not (Test-Path $Exe)) {
    throw "device-hub.exe が見つかりません: $Exe`n  先に 'cargo build --release -p device-hub' を実行するか、-Exe でパスを指定してください。"
}
$Exe = (Resolve-Path $Exe).Path
$destDir = Join-Path $env:LOCALAPPDATA 'DeviceHub\bin'
$dest = Join-Path $destDir 'device-hub.exe'

if ($Apply -and -not $Yes) {
    Write-Host "G HUB を停止・アンインストールします。" -ForegroundColor Yellow
    Write-Host "Device HUB で PRO 2 の DPI / ポーリングレートが実機に反映されることを確認済みですか?"
    $a = Read-Host "確認済みなら 'yes' と入力"
    if ($a -ne 'yes') { Write-Host '中止しました。'; exit 1 }
}

# 1-2. インストールと自動起動
Step "Device HUB を $destDir に配置し、自動起動を登録"
Do-Or-Say "copy $Exe -> $dest; HKCU Run に DeviceHub を登録" {
    New-Item -ItemType Directory -Force -Path $destDir | Out-Null
    # 常駐中の旧プロセスがあるとコピーできないので先に止める。
    Get-Process -Name 'device-hub' -ErrorAction SilentlyContinue | Stop-Process -Force
    Copy-Item $Exe $dest -Force
    # アプリ内の「常駐・起動」設定と同じ登録 (HKCU の Run、値名 DeviceHub、--background)。
    $cmd = '"' + $dest + '" --background'
    & reg.exe add 'HKCU\Software\Microsoft\Windows\CurrentVersion\Run' /v DeviceHub /t REG_SZ /d $cmd /f | Out-Null
    if ($LASTEXITCODE -ne 0) { throw '自動起動の登録に失敗しました' }
    Write-Host "  登録しました (次回ログオンから --background で起動します)"
}

# 3. G HUB の停止
Step 'G HUB のプロセス/サービスを停止'
$procs = Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -like 'lghub*' }
$svcs = Get-Service -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like '*LGHUB*' -or $_.Name -like '*LGHUB*' }
if (-not $procs -and -not $svcs) { Write-Host '  G HUB は動いていません' }
foreach ($s in $svcs) {
    Do-Or-Say "stop service $($s.Name)" { Stop-Service -Name $s.Name -Force -ErrorAction SilentlyContinue }
}
foreach ($p in $procs) {
    Do-Or-Say "kill $($p.ProcessName) (pid $($p.Id))" { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
}

# 4. アンインストール
if ($SkipUninstall) {
    Step 'アンインストールはスキップ (-SkipUninstall)'
} else {
    Step 'G HUB をアンインストール'
    $winget = Get-Command winget -ErrorAction SilentlyContinue
    $viaWinget = $false
    if ($winget) {
        & winget list --id Logitech.GHUB -e --accept-source-agreements *> $null
        $viaWinget = ($LASTEXITCODE -eq 0)
    }
    if ($viaWinget) {
        Do-Or-Say 'winget uninstall --id Logitech.GHUB -e --silent' {
            & winget uninstall --id Logitech.GHUB -e --silent
            if ($LASTEXITCODE -ne 0) { Write-Warning "winget が $LASTEXITCODE を返しました。設定 → アプリ から手動で削除してください。" }
        }
    } else {
        $keys = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
                'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
                'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
        $entry = Get-ItemProperty $keys -ErrorAction SilentlyContinue |
            Where-Object { $_.DisplayName -like '*G HUB*' -or $_.DisplayName -like '*LGHUB*' } |
            Select-Object -First 1
        if ($entry -and $entry.UninstallString) {
            Write-Host "  アンインストーラ: $($entry.UninstallString)"
            Write-Host '  サイレント指定が分からないため、アンインストーラの画面を開きます。画面の指示で完了してください。'
            Do-Or-Say "run $($entry.UninstallString)" {
                Start-Process -FilePath 'cmd.exe' -ArgumentList '/c', $entry.UninstallString -Wait
            }
        } else {
            Write-Host '  G HUB のインストールが見つかりません (すでに削除済み?)'
        }
    }
}

# 5. データの退避 (削除はしない)
if ($BackupData) {
    Step 'G HUB の設定フォルダを退避 (移動のみ)'
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $backup = Join-Path ([Environment]::GetFolderPath('MyDocuments')) "GHUB-backup-$stamp"
    $dirs = @(
        (Join-Path $env:APPDATA 'LGHUB'),
        (Join-Path $env:LOCALAPPDATA 'LGHUB'),
        (Join-Path $env:ProgramData 'LGHUB')
    )
    foreach ($d in $dirs) {
        if (Test-Path $d) {
            $name = ($d -replace '[:\\]', '_')
            Do-Or-Say "move $d -> $backup\$name" {
                New-Item -ItemType Directory -Force -Path $backup | Out-Null
                Move-Item $d (Join-Path $backup $name)
            }
        }
    }
    if ($Apply) { Write-Host "  退避先: $backup" }
}

Step '完了'
if ($Apply) {
    Write-Host "Device HUB を起動します (以後はログオン時に自動で最小化起動)。"
    Start-Process $dest
} else {
    Write-Host 'ドライランでした。実行するには -Apply を付けてください。'
}
