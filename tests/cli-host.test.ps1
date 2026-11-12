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
using System.Collections.Generic;
using System.Reflection;
using System.Threading;
public static class CodexCliProcessChecks {
    public static void Run(string hostExe, string mockExe, string[] cases, bool[] expected) {
        MethodInfo check = Assembly.LoadFile(hostExe).GetType("CodexPetHost")
            .GetMethod("IsCli", BindingFlags.NonPublic | BindingFlags.Static);
        List<string> failures = new List<string>();
        for (int i = 0; i < cases.Length; i++) {
            string args = cases[i];
            ProcessStartInfo info = new ProcessStartInfo(mockExe, args);
            info.UseShellExecute = false; info.CreateNoWindow = true;
            info.WindowStyle = ProcessWindowStyle.Hidden;
            using (Process p = Process.Start(info)) {
                try {
                    Thread.Sleep(150);
                    bool actual = (bool)check.Invoke(null, new object[] { p });
                    if (actual != expected[i]) failures.Add(args + " (expected " + expected[i] + ", got " + actual + ")");
                } finally {
                    if (!p.HasExited) { p.Kill(); p.WaitForExit(5000); }
                }
            }
        }
        if (failures.Count > 0) throw new Exception("Incorrect CLI classifications: " + String.Join("; ", failures));
    }
}
'@
$cases=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'fixtures/cli-command-lines.json') -Raw -Encoding UTF8 | ConvertFrom-Json
[CodexCliProcessChecks]::Run($hostExe,$mockExe,[string[]]@($cases | ForEach-Object {$_.arguments}),[bool[]]@($cases | ForEach-Object {$_.expected}))
Write-Output ('CLI host: '+$cases.Count+' Windows command-line classification cases passed.')
