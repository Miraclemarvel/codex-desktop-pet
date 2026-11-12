using System;
using System.Diagnostics;
using System.IO;
using System.Management;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Threading;

// A windowless launcher. The Electron pet only runs for a live Codex session.
internal static class CodexPetHost
{
    static string Option(string[] args, string name, string fallback)
    {
        int i = Array.IndexOf(args, name);
        return i >= 0 && i + 1 < args.Length ? args[i + 1] : fallback;
    }
    static bool Alive(Process p) { try { return p != null && !p.HasExited; } catch { return false; } }
    static string Quote(string s) { return "\"" + s.Replace("\"", "\\\"") + "\""; }
    static string Json(string s) { return "\"" + s.Replace("\\", "\\\\").Replace("\"", "\\\"").Replace("\r", " ").Replace("\n", " ") + "\""; }
    static string Key(Process p) { return p.Id + ":" + p.StartTime.ToUniversalTime().Ticks; }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    static extern IntPtr CommandLineToArgvW(string command, out int count);
    [DllImport("kernel32.dll")]
    static extern IntPtr LocalFree(IntPtr memory);

    static bool IsCliCommand(string command)
    {
        if (String.IsNullOrWhiteSpace(command)) return false;
        int count;
        IntPtr argv = CommandLineToArgvW(command, out count);
        if (argv == IntPtr.Zero) return false;
        try
        {
            string[] args = new string[count];
            for (int i = 0; i < count; i++) args[i] = Marshal.PtrToStringUni(Marshal.ReadIntPtr(argv, i * IntPtr.Size));
            string[] values = { "--config", "--enable", "--disable", "--remote", "--remote-auth-token-env",
                "--image", "--model", "--local-provider", "--profile", "--sandbox", "--cd", "--add-dir", "--ask-for-approval",
                "-c", "-i", "-m", "-p", "-s", "-C", "-a" };
            string[] excluded = { "app-server", "exec-server", "mcp-server", "mcp", "completion", "login", "logout" };
            // Only the first positional argument can be the top-level command.
            // Prompts and arguments after exec/resume/etc. must not affect it.
            for (int i = 1; i < args.Length; i++)
            {
                string arg = args[i];
                if (arg == "--") return true;
                if (Array.IndexOf(values, arg) >= 0)
                {
                    if (++i >= args.Length) return false;
                    continue;
                }
                // --option=value and attached short values (-pname, -Cpath)
                // are already one argv element, so consume no following value.
                if (arg.StartsWith("-", StringComparison.Ordinal) && arg != "-") continue;
                return Array.IndexOf(excluded, arg) < 0;
            }
            return true;
        }
        finally { LocalFree(argv); }
    }

    static bool IsCli(Process p)
    {
        try
        {
            if (!String.Equals(p.ProcessName, "codex", StringComparison.OrdinalIgnoreCase)) return false;
            if (p.MainModule.FileName.IndexOf("\\OpenAI.Codex_", StringComparison.OrdinalIgnoreCase) >= 0) return false;
            using (ManagementObject entry = new ManagementObject("Win32_Process.Handle='" + p.Id + "'"))
            {
                string command = (string)entry["CommandLine"];
                return IsCliCommand(command);
            }
        }
        catch { return false; }
    }

    static Process FindCodex(bool cliOnly)
    {
        int session = Process.GetCurrentProcess().SessionId;
        foreach (string name in new string[] { "ChatGPT", "Codex" })
        {
            foreach (Process p in Process.GetProcessesByName(name))
            {
                try
                {
                    string exe = p.MainModule.FileName;
                    if (!cliOnly && p.SessionId == session && p.MainWindowHandle != IntPtr.Zero &&
                        exe.IndexOf("\\OpenAI.Codex_", StringComparison.OrdinalIgnoreCase) >= 0)
                        return p;
                }
                catch { }
                p.Dispose();
            }
        }
        foreach (Process p in Process.GetProcessesByName("codex"))
        {
            try { if (p.SessionId == session && IsCli(p)) return p; } catch { }
            p.Dispose();
        }
        return null;
    }

    static Process ExistingPet(string exe)
    {
        foreach (Process p in Process.GetProcessesByName(Path.GetFileNameWithoutExtension(exe)))
        {
            try
            {
                if (String.Equals(p.MainModule.FileName, exe, StringComparison.OrdinalIgnoreCase) &&
                    p.MainWindowTitle == "Codex 白龙桌宠") return p;
            }
            catch { }
            p.Dispose();
        }
        return null;
    }

    static void StopPet(Process pet)
    {
        if (!Alive(pet)) return;
        try
        {
            pet.CloseMainWindow();
            // The pet also checks its exact owner PID every second.
            if (!pet.WaitForExit(2500) && Alive(pet)) pet.Kill();
        }
        catch { }
    }

    static void State(string file, string status, Process owner, Process pet, string error)
    {
        if (String.IsNullOrEmpty(file)) return;
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(file));
            File.WriteAllText(file, "{\"status\":" + Json(status) +
                ",\"hostPid\":" + Process.GetCurrentProcess().Id +
                ",\"codexPid\":" + (Alive(owner) ? owner.Id.ToString() : "null") +
                ",\"petPid\":" + (Alive(pet) ? pet.Id.ToString() : "null") +
                ",\"error\":" + (error == null ? "null" : Json(error)) + "}");
        }
        catch { }
    }

    [STAThread]
    static int Main(string[] args)
    {
        string sessionName = Option(args, "--session", "default");
        string identity = WindowsIdentity.GetCurrent().User.Value + "." + sessionName;
        string stopName = @"Local\CodexDesktopPetHost.Stop." + identity;
        if (Array.IndexOf(args, "--stop") >= 0)
        {
            try { using (EventWaitHandle stop = EventWaitHandle.OpenExisting(stopName)) stop.Set(); }
            catch (WaitHandleCannotBeOpenedException) { }
            return 0;
        }
        string stateFile = Option(args, "--state-path", Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "Codex Desktop Pet", "host-state.json"));
        if (Array.IndexOf(args, "--probe") >= 0)
        {
            using (Process owner = FindCodex(Array.IndexOf(args, "--cli") >= 0)) State(stateFile, "probe", owner, null, null);
            return 0;
        }
        bool first;
        using (Mutex mutex = new Mutex(true, @"Local\CodexDesktopPetHost." + identity, out first))
        {
            if (!first) return 0;
            using (EventWaitHandle stop = new EventWaitHandle(false, EventResetMode.ManualReset, stopName))
            {
                string petExe = Path.GetFullPath(Option(args, "--pet-exe", Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "CodexPet.exe")));
                string testData = Option(args, "--pet-user-data", null);
                int fixedPid;
                Int32.TryParse(Option(args, "--codex-pid", "0"), out fixedPid);
                Process owner = null, pet = null;
                string ownerKey = null;
                bool dismissed = false;
                State(stateFile, "waiting", null, null, null);
                // Let an installer release Electron's single-instance lock first.
                if (stop.WaitOne(600)) return 0;
                try
                {
                    while (!stop.WaitOne(0))
                    {
                        if (!Alive(owner))
                        {
                            StopPet(pet);
                            if (pet != null) pet.Dispose();
                            if (owner != null) owner.Dispose();
                            pet = null; owner = null;
                            try { owner = fixedPid > 0 ? Process.GetProcessById(fixedPid) : FindCodex(Array.IndexOf(args, "--cli") >= 0); } catch { }
                            if (!Alive(owner))
                            {
                                State(stateFile, "waiting", null, null, null);
                                if (fixedPid > 0) break;
                                if (stop.WaitOne(2000)) break;
                                continue;
                            }
                            string next = Key(owner);
                            if (next != ownerKey) { ownerKey = next; dismissed = false; }
                        }
                        if (pet != null && !Alive(pet))
                        {
                            // A manual exit lasts for this Codex session; do not reopen it.
                            dismissed = true;
                            pet.Dispose(); pet = null;
                            State(stateFile, "dismissed", owner, null, null);
                        }
                        if (pet == null && !dismissed)
                        {
                            try
                            {
                                string childArgs = "--codex-pid " + owner.Id;
                                if (Array.IndexOf(args, "--cli") >= 0 || IsCli(owner))
                                    childArgs += " --cli --codex-since " + ((long)(owner.StartTime.ToUniversalTime() - new DateTime(1970, 1, 1)).TotalMilliseconds) + " --codex-exe " + Quote(owner.MainModule.FileName);
                                if (testData != null) childArgs += " --test-user-data " + Quote(testData);
                                ProcessStartInfo info = new ProcessStartInfo(petExe, childArgs);
                                info.UseShellExecute = false; info.CreateNoWindow = true;
                                info.WindowStyle = ProcessWindowStyle.Hidden;
                                info.WorkingDirectory = Path.GetDirectoryName(petExe);
                                pet = Process.Start(info);
                                if (pet.WaitForExit(800))
                                {
                                    pet.Dispose(); pet = ExistingPet(petExe);
                                    if (pet == null) dismissed = true;
                                }
                                State(stateFile, dismissed ? "dismissed" : "running", owner, pet, null);
                            }
                            catch (Exception e)
                            {
                                State(stateFile, "error", owner, null, e.Message);
                                if (stop.WaitOne(5000)) break;
                            }
                        }
                        if (stop.WaitOne(1000)) break;
                    }
                }
                finally
                {
                    if (!Alive(owner)) StopPet(pet);
                    State(stateFile, "stopped", owner, pet, null);
                    if (pet != null) pet.Dispose();
                    if (owner != null) owner.Dispose();
                }
            }
        }
        return 0;
    }
}
