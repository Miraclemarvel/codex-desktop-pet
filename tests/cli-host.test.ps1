$ErrorActionPreference='Stop'
$petRoot=Split-Path $PSScriptRoot
$hostExe=Join-Path $petRoot 'native/CodexPetHost.exe'
$testDir=Join-Path $petRoot ('outputs/cli-host-tests/'+[guid]::NewGuid().ToString('N')+'/mock bin')
New-Item -ItemType Directory -Path $testDir -Force | Out-Null
$mockExe=Join-Path $testDir 'codex.exe'
$source=Join-Path $testDir 'mock.cs'
'using System.Threading; class MockCodex { static void Main(string[] args) { Thread.Sleep(30000); } }' | Set-Content -LiteralPath $source -Encoding ASCII
$compiler=Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
& $compiler /nologo /target:exe "/out:$mockExe" $source
if($LASTEXITCODE -ne 0){throw 'CLI fixture compilation failed.'}
Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Reflection;
using System.Threading;
public static class CodexCliProcessChecks {
    public static void Run(string hostExe, string mockExe) {
        MethodInfo check = Assembly.LoadFile(hostExe).GetType("CodexPetHost")
            .GetMethod("IsCli", BindingFlags.NonPublic | BindingFlags.Static);
        foreach (string args in new [] { "", "exec", "app-server", "exec-server", "mcp-server", "-c x=y app-server" }) {
            ProcessStartInfo info = new ProcessStartInfo(mockExe, args);
            info.UseShellExecute = false; info.CreateNoWindow = true;
            info.WindowStyle = ProcessWindowStyle.Hidden;
            using (Process p = Process.Start(info)) {
                try {
                    Thread.Sleep(150);
                    bool actual = (bool)check.Invoke(null, new object[] { p });
                    bool expected = args == "" || args == "exec";
                    if (actual != expected) throw new Exception("Incorrect CLI classification: " + args);
                } finally {
                    if (!p.HasExited) { p.Kill(); p.WaitForExit(5000); }
                }
            }
        }
    }
}
'@
[CodexCliProcessChecks]::Run($hostExe,$mockExe)
Write-Output 'CLI host: interactive/exec accepted; app-server/exec-server/mcp-server rejected (6 cases).'
