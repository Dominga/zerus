param(
    [ValidateSet('Controller', 'Child')][string]$Mode = 'Controller',
    [string]$Stage,
    [string]$TestExe,
    [ValidatePattern('^[a-z0-9-]+$')][string]$Label = 'candidate'
)
# Disposable hosted runner only. Adapted from windows-registration-native-user.ps1.
# The fresh ordinary account runs native unit controls, never registration or input.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
if ($Mode -eq 'Controller' -and ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:GITHUB_EVENT_NAME -ne 'workflow_dispatch')) {
    throw 'This standard-user fixture requires an explicit disposable hosted run'
}

$captureSource = @'
using System;
using System.Diagnostics;
using System.IO;
using System.ServiceProcess;
using System.Text;
using System.Threading.Tasks;
public static class StartRefusalCapture {
    public static Task<string> Read(Stream stream) {
        return Task.Run(() => {
            var bytes = new byte[65537]; int count = 0;
            while (count < bytes.Length) {
                int n = stream.Read(bytes, count, bytes.Length - count);
                if (n == 0) return new UTF8Encoding(false, true).GetString(bytes, 0, count);
                count += n;
            }
            throw new InvalidDataException("capture exceeded 64 KiB");
        });
    }
    public static Task<bool> ReadyLogonService(string phases) {
        return Task.Run(() => {
            var watch = Stopwatch.StartNew();
            using (var service = new ServiceController("seclogon")) {
                var before = service.Status;
                File.AppendAllText(phases, "before " + before + "\n");
                if (before == ServiceControllerStatus.Stopped) service.Start();
                else if (before != ServiceControllerStatus.Running && before != ServiceControllerStatus.StartPending)
                    throw new InvalidOperationException("Unexpected Secondary Logon state");
                service.WaitForStatus(ServiceControllerStatus.Running, TimeSpan.FromSeconds(20));
                File.AppendAllText(phases, "running " + watch.ElapsedMilliseconds + "\n");
                return true;
            }
        });
    }
    public static Task<bool> Start(Process child, string phases) {
        return Task.Run(() => {
            File.AppendAllText(phases, "start_enter\n");
            bool started = child.Start();
            File.AppendAllText(phases, "start_return\n");
            return started;
        });
    }
    public static Task Stop(Process child) {
        // A native termination request can itself stall. Keep it off the
        // caller deadline and retain the original Process until a real wait.
        return Task.Run(() => { child.Kill(); child.WaitForExit(); });
    }
}
'@

$assembly = [Reflection.Assembly]::Load('System.Web.Extensions, Version=4.0.0.0, Culture=neutral, PublicKeyToken=31bf3856ad364e35')
$json = [Activator]::CreateInstance($assembly.GetType('System.Web.Script.Serialization.JavaScriptSerializer', $true))
$json.MaxJsonLength = 524288
function Write-Report($Path, $Value) {
    [IO.File]::WriteAllText($Path, $json.Serialize($Value), [Text.UTF8Encoding]::new($false))
}
if ($Mode -eq 'Child') {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $admin = @($identity.Groups | Where-Object { $_.Value -eq 'S-1-5-32-544' }).Count -ne 0
    if ($admin) { throw 'A genuine standard user without an Administrators SID is required' }
    foreach ($entry in @(Get-ChildItem Env: | Where-Object { $_.Name.StartsWith('HGS_') -or $_.Name.StartsWith('DSH_') })) {
        Remove-Item ('Env:' + $entry.Name)
    }
    foreach ($leaf in @('home','local','roaming','tmp')) { [void][IO.Directory]::CreateDirectory([IO.Path]::Combine($Stage,$leaf)) }
    $env:HOME = [IO.Path]::Combine($Stage,'home')
    $env:USERPROFILE = $env:HOME
    $env:LOCALAPPDATA = [IO.Path]::Combine($Stage,'local')
    $env:APPDATA = [IO.Path]::Combine($Stage,'roaming')
    $env:TEMP = [IO.Path]::Combine($Stage,'tmp'); $env:TMP = $env:TEMP
    $volume = [IO.DriveInfo]::new([IO.Path]::GetPathRoot($env:TEMP))
    if ($volume.DriveFormat -ne 'NTFS') { throw 'The native qualification requires NTFS' }
    $report = [ordered]@{ sid=$identity.User.Value; administrator_sid_present=$admin; filesystem=$volume.DriveFormat; calls=@(); complete=$false; settled=$false }
    $reportPath = [IO.Path]::Combine($Stage,'result.json')
    [void][Reflection.Assembly]::Load([IO.File]::ReadAllBytes([IO.Path]::Combine($Stage,'capture.dll')))
    $filters = @(
        @('state::input::task_refusal::windows_tests::',6),
        @('tasks::dispatch::mode_refusal_tests::',2),
        @('state::task_controls::writer::tests::',2),
        @('state::repositories::directory::windows_publication::tests::original_handle_publication_keeps_pins_and_the_original_file',1),
        @('state::repositories::directory::windows_publication::tests::no_replace_and_original_reader_refusal_preserve_the_previous_bytes',1)
    )
    foreach ($filter in $filters) {
        $child = [Diagnostics.Process]::new()
        $child.StartInfo.FileName = [IO.Path]::Combine($Stage,'hgs-tests.exe')
        $child.StartInfo.Arguments = $filter[0] + ' --nocapture --test-threads=1'
        $child.StartInfo.UseShellExecute=$false; $child.StartInfo.CreateNoWindow=$true
        $child.StartInfo.RedirectStandardInput=$true; $child.StartInfo.RedirectStandardOutput=$true; $child.StartInfo.RedirectStandardError=$true
        [void]$child.Start(); $originalHandle=$child.Handle
        $call=[ordered]@{ filter=$filter[0]; pid=$child.Id; birth=$child.StartTime.ToUniversalTime().ToFileTimeUtc().ToString(); physical_wait=$false }
        $report.calls += $call; Write-Report $reportPath $report
        $child.StandardInput.Close()
        $out=[StartRefusalCapture]::Read($child.StandardOutput.BaseStream); $err=[StartRefusalCapture]::Read($child.StandardError.BaseStream)
        if (-not $child.WaitForExit(60000)) {
            $stop=[StartRefusalCapture]::Stop($child); try { [void]$stop.Wait(3000) } catch { }
            throw 'Original native test child exceeded its deadline'
        }
        $call.physical_wait=$true; $call.exit=$child.ExitCode
        if (-not [Threading.Tasks.Task]::WaitAll(@($out,$err),3000)) { throw 'Original native test output remains pending' }
        $call.stdout=$out.GetAwaiter().GetResult(); $call.stderr=$err.GetAwaiter().GetResult(); $child.Dispose()
        Write-Report $reportPath $report
        $match=[regex]::Match($call.stdout,'test result: ok\. (\d+) passed; 0 failed;')
        if ($call.exit -ne 0 -or -not $match.Success -or [int]$match.Groups[1].Value -lt [int]$filter[1]) { $report.settled=$true; Write-Report $reportPath $report; throw 'A required standard-user native control failed or did not run' }
    }
    $report.complete=$true; $report.settled=$true; Write-Report $reportPath $report
    return
}
$source=(Resolve-Path $TestExe).Path
$nonce=[Guid]::NewGuid().ToString('N'); $userName='zrf'+$nonce.Substring(0,10)
if (-not $Stage) { $Stage=[IO.Path]::Combine([Environment]::GetFolderPath('CommonApplicationData'),'zerus-refusal-'+$nonce) }
$Stage=[IO.Path]::GetFullPath($Stage)
if (Test-Path $Stage) { throw 'Native fixture stage must not exist' }
[void][IO.Directory]::CreateDirectory($Stage)
$reportRoot=Join-Path (Join-Path $env:GITHUB_WORKSPACE 'artifacts/test-results/start-refusal') $Label
[void][IO.Directory]::CreateDirectory($reportRoot)
$bytes=New-Object byte[] 32; [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
$password=[Convert]::ToBase64String($bytes)+'!aA1'; $secure=ConvertTo-SecureString $password -AsPlainText -Force
$user=$null; $outer=$null; $complete=$false; $settled=$false
try {
    $capturePath=Join-Path $Stage 'capture.dll'
    $serviceAssembly=[Reflection.Assembly]::Load('System.ServiceProcess, Version=4.0.0.0, Culture=neutral, PublicKeyToken=b03f5f7f11d50a3a')
    Add-Type -TypeDefinition $captureSource -ReferencedAssemblies @('System.dll','System.Core.dll',$serviceAssembly.Location) -OutputAssembly $capturePath
    $user=New-LocalUser -Name $userName -Password $secure -Description 'Disposable Zerus Start refusal CI fixture' -AccountNeverExpires
    Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $user
    & icacls.exe $Stage /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' "*$($user.SID.Value):(OI)(CI)M" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Private standard-user stage ACL failed' }
    Copy-Item $source (Join-Path $Stage 'hgs-tests.exe')
    Copy-Item $PSCommandPath (Join-Path $Stage 'fixture.ps1')
    $digest=(Get-FileHash $source -Algorithm SHA256).Hash.ToLowerInvariant()
    if ((Get-FileHash (Join-Path $Stage 'hgs-tests.exe') -Algorithm SHA256).Hash.ToLowerInvariant() -ne $digest) { throw 'Original native image copy changed' }
    [void][Reflection.Assembly]::Load([IO.File]::ReadAllBytes($capturePath))
    $ready=[StartRefusalCapture]::ReadyLogonService((Join-Path $Stage 'logon-phases.txt'))
    if (-not $ready.Wait(30000) -or -not $ready.Result) { throw 'Disposable logon service setup remains pending' }
    $entry="& ([ScriptBlock]::Create([IO.File]::ReadAllText('"+(Join-Path $Stage 'fixture.ps1').Replace("'","''")+"'))) -Mode Child -Stage '"+$Stage.Replace("'","''")+"'"
    $arguments='-NoLogo -NoProfile -NonInteractive -EncodedCommand '+[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($entry))
    $program=Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    if ($program.Length+$arguments.Length+3 -ge 1024) { throw 'Credentialed native launch exceeds its bound' }
    $outer=[Diagnostics.Process]::new(); $outer.StartInfo.FileName=$program; $outer.StartInfo.Arguments=$arguments
    $outer.StartInfo.WorkingDirectory=$Stage; $outer.StartInfo.UseShellExecute=$false; $outer.StartInfo.CreateNoWindow=$true
    # This is filesystem/token qualification with explicit private directories,
    # not HKCU or cold-profile acceptance. The hosted profile-load stall is
    # documented in zerus-5wmh.66.3.27.4.1. No-load still logs on the real new
    # user; its kernel SID must match and no Administrators SID may be present.
    $outer.StartInfo.UserName=$userName; $outer.StartInfo.Domain=$env:COMPUTERNAME; $outer.StartInfo.Password=$secure; $outer.StartInfo.LoadUserProfile=$false
    $outer.StartInfo.RedirectStandardInput=$true; $outer.StartInfo.RedirectStandardOutput=$true; $outer.StartInfo.RedirectStandardError=$true
    $starting=[StartRefusalCapture]::Start($outer,(Join-Path $Stage 'launch-phases.txt'))
    if (-not $starting.Wait(20000) -or -not $starting.Result) { throw 'Credentialed original launch remains pending' }
    $originalHandle=$outer.Handle; $password=$null; $secure=$null
    $outer.StandardInput.Close(); $out=[StartRefusalCapture]::Read($outer.StandardOutput.BaseStream); $err=[StartRefusalCapture]::Read($outer.StandardError.BaseStream)
    if (-not $outer.WaitForExit(330000)) {
        $stop=[StartRefusalCapture]::Stop($outer); try { [void]$stop.Wait(3000) } catch { }
        throw 'Original standard-user wrapper exceeded its deadline'
    }
    if (-not [Threading.Tasks.Task]::WaitAll(@($out,$err),3000)) { throw 'Original wrapper output remains pending' }
    $launch=[ordered]@{ physical_wait=$true; exit=$outer.ExitCode; stdout=$out.GetAwaiter().GetResult(); stderr=$err.GetAwaiter().GetResult(); image_sha256=$digest; load_user_profile=$outer.StartInfo.LoadUserProfile }
    Write-Report (Join-Path $reportRoot 'launch.json') $launch
    $outer.Dispose(); $outer=$null
    $text=[IO.File]::ReadAllText((Join-Path $Stage 'result.json'))
    if ($text.Length -gt 500000) { throw 'Native report exceeds its bound' }
    [IO.File]::WriteAllText((Join-Path $reportRoot 'standard-user.json'),$text)
    $result=$json.DeserializeObject($text)
    $complete=$launch.exit -eq 0 -and $result.complete -eq $true -and $result.sid -eq $user.SID.Value -and $result.administrator_sid_present -eq $false -and $result.filesystem -eq 'NTFS' -and $result.calls.Count -eq 5
    $settled=$result.settled -eq $true -and @($result.calls | Where-Object { $_.physical_wait -ne $true }).Count -eq 0
    if (-not $complete) { throw 'Standard-user NTFS qualification failed' }
} finally {
    foreach ($name in @('result.json','logon-phases.txt','launch-phases.txt')) {
        $path=Join-Path $Stage $name
        if ([IO.File]::Exists($path)) {
            $file=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
            try {
                if ($file.Length -gt 524288) { throw 'Retained fixture evidence exceeds bound' }
                $buffer=New-Object byte[] ([int]$file.Length); $offset=0
                while ($offset -lt $buffer.Length) { $n=$file.Read($buffer,$offset,$buffer.Length-$offset); if ($n -le 0) { throw 'Incomplete fixture evidence' }; $offset+=$n }
                [IO.File]::WriteAllBytes((Join-Path $reportRoot $name),$buffer)
            } finally { $file.Dispose() }
        }
    }
    if ($settled) {
        $until=[DateTime]::UtcNow.AddSeconds(10)
        do { $profile=Get-CimInstance Win32_UserProfile -Filter "SID='$($user.SID.Value)'"; if (-not $profile.Loaded) { break }; Start-Sleep -Milliseconds 100 } while ([DateTime]::UtcNow -lt $until)
        if ($profile.Loaded) { throw 'Original fixture profile remains loaded' }
        if ($profile) { $profile | Remove-CimInstance }
        Remove-LocalUser -SID $user.SID
        Remove-Item -LiteralPath $Stage -Recurse
        Write-Report (Join-Path $reportRoot 'cleanup.json') @{account_removed=$true;profile_absent=$true;stage_removed=$true}
    } else { Write-Output 'Incomplete fixture retained only on the disposable runner; no effect replayed' }
}
