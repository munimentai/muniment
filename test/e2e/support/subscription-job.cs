using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

// A suspended root joins the job before it can create descendants.
public sealed class SubscriptionJob : IDisposable
{
    [StructLayout(LayoutKind.Sequential)]
    struct StartupInfo {
        public int cb;
        public IntPtr reserved, desktop, title;
        public int x, y, width, height, xChars, yChars, fill, flags;
        public short show, reservedSize;
        public IntPtr reservedBytes, stdin, stdout, stderr;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct ProcessInfo { public IntPtr process, thread; public int pid, tid; }
    [StructLayout(LayoutKind.Sequential)]
    struct BasicLimits {
        public long processTime, jobTime;
        public uint flags;
        public UIntPtr minWorkingSet, maxWorkingSet;
        public uint activeLimit;
        public UIntPtr affinity;
        public uint priority, scheduling;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct IoCounters { public ulong readOps, writeOps, otherOps, readBytes, writeBytes, otherBytes; }
    [StructLayout(LayoutKind.Sequential)]
    struct ExtendedLimits {
        public BasicLimits basic;
        public IoCounters io;
        public UIntPtr processMemory, jobMemory, peakProcessMemory, peakJobMemory;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct Accounting {
        public long userTime, kernelTime, periodUserTime, periodKernelTime;
        public uint faults, totalProcesses, activeProcesses, terminatedProcesses;
    }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateJobObject(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool SetInformationJobObject(IntPtr job, int kind, ref ExtendedLimits info, int length);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool QueryInformationJobObject(IntPtr job, int kind, out Accounting info, int length, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool TerminateJobObject(IntPtr job, uint code);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CreateProcess(string executable, StringBuilder command, IntPtr processAttributes,
        IntPtr threadAttributes, bool inherit, uint flags, IntPtr environment, string cwd,
        ref StartupInfo startup, out ProcessInfo process);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool TerminateProcess(IntPtr process, uint code);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll")]
    static extern IntPtr GetStdHandle(int kind);
    [DllImport("kernel32.dll")]
    static extern bool CloseHandle(IntPtr handle);

    IntPtr job, root;
    public int Pid { get; private set; }
    public bool Exited { get { return WaitForSingleObject(root, 0) == 0; } }

    static void Check(bool success) {
        if (!success) throw new Win32Exception(Marshal.GetLastWin32Error());
    }
    static string Quote(string value) {
        var result = new StringBuilder("\"");
        int slashes = 0;
        foreach (char c in value) {
            if (c == '\\') { slashes++; continue; }
            result.Append('\\', c == '"' ? slashes * 2 + 1 : slashes);
            result.Append(c);
            slashes = 0;
        }
        return result.Append('\\', slashes * 2).Append('"').ToString();
    }
    public SubscriptionJob(string executable, string[] args, string name) {
        // Use the caller's default DACL. The same user opens the job after an MSI restart.
        job = CreateJobObject(IntPtr.Zero, name);
        Check(job != IntPtr.Zero);
        if (Marshal.GetLastWin32Error() == 183) {
            CloseHandle(job);
            job = IntPtr.Zero;
            throw new InvalidOperationException("The native probe job name already exists.");
        }
        var process = new ProcessInfo();
        try {
            var limits = new ExtendedLimits();
            // Set only KILL_ON_JOB_CLOSE. Neither breakaway flag nor UI restrictions apply.
            limits.basic.flags = 0x2000;
            Check(SetInformationJobObject(job, 9, ref limits, Marshal.SizeOf(limits)));
            var startup = new StartupInfo();
            startup.cb = Marshal.SizeOf(startup);
            startup.flags = 0x100;
            startup.stdin = GetStdHandle(-10);
            startup.stdout = GetStdHandle(-11);
            startup.stderr = GetStdHandle(-12);
            var command = new StringBuilder(Quote(executable));
            foreach (string arg in args ?? new string[0]) command.Append(' ').Append(Quote(arg));
            Check(CreateProcess(executable, command, IntPtr.Zero, IntPtr.Zero, true, 4,
                IntPtr.Zero, null, ref startup, out process));
            root = process.process;
            Pid = process.pid;
            if (!AssignProcessToJobObject(job, root)) {
                int error = Marshal.GetLastWin32Error();
                throw new Win32Exception(error, "AssignProcessToJobObject failed. GetLastError=" + error + ".");
            }
            Check(ResumeThread(process.thread) != uint.MaxValue);
        } catch {
            if (root != IntPtr.Zero) {
                TerminateProcess(root, 1);
                WaitForSingleObject(root, 15000);
            }
            Dispose();
            throw;
        } finally {
            if (process.thread != IntPtr.Zero) CloseHandle(process.thread);
        }
    }
    public void Stop(int timeoutMs) {
        Check(TerminateJobObject(job, 1));
        var clock = Stopwatch.StartNew();
        for (;;) {
            Accounting info;
            Check(QueryInformationJobObject(job, 1, out info, Marshal.SizeOf(typeof(Accounting)), IntPtr.Zero));
            if (info.activeProcesses == 0) return;
            if (clock.ElapsedMilliseconds >= timeoutMs)
                throw new TimeoutException("The native probe descendants did not stop before the deadline.");
            Thread.Sleep(25);
        }
    }
    public void Dispose() {
        if (job != IntPtr.Zero) { CloseHandle(job); job = IntPtr.Zero; }
        if (root != IntPtr.Zero) { CloseHandle(root); root = IntPtr.Zero; }
    }
}
