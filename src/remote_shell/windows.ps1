# Fixed transport; only __HGS_ARGUMENTS__ is replaced with encoded JSON argv.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$p=$null
try {
# Avoid implicit first-cmdlet discovery on a cold Windows PowerShell 5.1 profile.
$PSModuleAutoLoadingPreference='None'
Import-Module -Name "$PSHOME\Modules\Microsoft.PowerShell.Utility" -ErrorAction Stop
[string[]]$a=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('__HGS_ARGUMENTS__'))|ConvertFrom-Json
function q([string]$s) {'"'+[regex]::Replace($s,'(\\*)("|$)',{param($m)$b=$m.Groups[1].Value;if($m.Groups[2].Value -eq '"'){$b+$b+'\"'}else{$b+$b}})+'"'}
$i=[Diagnostics.ProcessStartInfo]::new();$i.FileName=[IO.Path]::Combine($env:USERPROFILE,'.local\bin\hgs.exe');$i.Arguments=(@(foreach($v in $a){q $v}))-join ' ';$i.UseShellExecute=$false
$pipe=[Console]::IsInputRedirected;$i.CreateNoWindow=$pipe;$i.RedirectStandardInput=$pipe;$i.RedirectStandardOutput=$pipe;$i.RedirectStandardError=$pipe
$p=[Diagnostics.Process]::new();$p.StartInfo=$i;if(-not$p.Start()){throw 'start refused'}
if($pipe){
$o=$p.StandardOutput.BaseStream.CopyToAsync([Console]::OpenStandardOutput());$e=$p.StandardError.BaseStream.CopyToAsync([Console]::OpenStandardError())
# Open the inherited SSH pipe with its actual async mode. ConsoleStream's
# fallback ReadAsync can stall after its first read on OpenSSH's async handle.
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class HgsSshInput {
[DllImport("kernel32.dll")] static extern IntPtr GetStdHandle(int n);
[DllImport("ntdll.dll")] static extern int NtQueryInformationFile(IntPtr h, [Out] IntPtr[] status, out uint mode, uint size, int info);
public static FileStream Open() {
var h=GetStdHandle(-10); uint mode;
if(NtQueryInformationFile(h,new IntPtr[2],out mode,4,16)!=0) throw new IOException("stdin mode unavailable");
return new FileStream(new SafeFileHandle(h,false),FileAccess.Read,1,(mode&48)==0);
}
}
'@
$s=[HgsSshInput]::Open();$b=[byte[]]::new(65536);$n=4;$pos=0;$header=$true;$closed=$false;$w=$null;$clock=[Diagnostics.Stopwatch]::StartNew();$r=$s.ReadAsync($b,0,4)
# Keep servicing the lease while output waits for a healthy slow consumer.
while($true){
$ended=$p.WaitForExit(20);if($ended -and $o.IsCompleted -and $e.IsCompleted){break};if($ended){[Threading.Thread]::Sleep(20)}
if($o.IsFaulted -or $e.IsFaulted -or $clock.ElapsedMilliseconds -gt 15000){throw 'connection lost'}
if($null -ne $w){if(-not$w.IsCompleted){continue};if(-not$ended){[void]$w.GetAwaiter().GetResult()};$w=$null;$r=$s.ReadAsync($b,0,4)}
if(-not$r.IsCompleted){continue}
$count=$r.GetAwaiter().GetResult();if($count -eq 0){throw 'connection closed'};$clock.Restart();$pos+=$count
if($pos -lt $n){$r=$s.ReadAsync($b,$pos,$n-$pos);continue}
$pos=0
if($header){$len=[BitConverter]::ToUInt32($b,0);if($len -eq [uint32]::MaxValue){$r=$s.ReadAsync($b,0,4);continue};if($len -eq 0){if($closed){throw 'duplicate EOF'};if(-not$ended){$p.StandardInput.Close()};$closed=$true;$r=$s.ReadAsync($b,0,4);continue};if($closed -or $len -gt 65536){throw 'invalid frame'};$n=[int]$len;$header=$false;$r=$s.ReadAsync($b,0,$n)}
else{if($ended){$r=$s.ReadAsync($b,0,4)}else{$w=$p.StandardInput.BaseStream.WriteAsync($b,0,$n)};$n=4;$header=$true}
}
[void]$o.GetAwaiter().GetResult();[void]$e.GetAwaiter().GetResult()
}else{$p.WaitForExit()}
exit $p.ExitCode
}catch{
# Never look up/adopt a process or stop a session host/agent. The original CLI
# helper is the only child this transport owns; native sessions are independent.
if($null -ne $p){try{if(-not$p.HasExited){$p.Kill();$p.WaitForExit()}}catch{}}
[Console]::Error.WriteLine('hgs: remote Windows invocation failed');exit 127
}
