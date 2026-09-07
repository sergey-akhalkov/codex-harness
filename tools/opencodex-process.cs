// Windows process-group containment for the OpenCodex integration.
// Contracts: learn.microsoft.com/windows/win32/procthread/job-objects and
// win32/api/winnt/ns-winnt-jobobject_extended_limit_information.
using System;
using System.Collections;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

namespace CodexHarness.SubscriptionProcess
{
    public sealed class RunResult
    {
        public string Status { get; set; }
        public int ExitCode { get; set; }
        public uint ProcessExitCode { get; set; }
        public uint ProcessId { get; set; }
        public ulong MemoryLimitBytes { get; set; }
        public ulong PeakJobMemoryBytes { get; set; }
        public long ElapsedMilliseconds { get; set; }
        public bool AssignedBeforeResume { get; set; }
    }

    public static class Runner
    {
        private const uint CREATE_SUSPENDED = 0x4;
        private const uint CREATE_UNICODE_ENVIRONMENT = 0x400;
        private const uint EXTENDED_STARTUPINFO_PRESENT = 0x80000;
        private const uint CREATE_NO_WINDOW = 0x8000000;
        private const uint STARTF_USESTDHANDLES = 0x100;
        private const uint JOB_OBJECT_LIMIT_JOB_MEMORY = 0x200;
        private const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000;
        private const uint WAIT_OBJECT_0 = 0;
        private const uint WAIT_TIMEOUT = 258;
        private const uint JOB_OBJECT_MSG_JOB_MEMORY_LIMIT = 10;
        private static readonly IntPtr InvalidHandle = new IntPtr(-1);

        [StructLayout(LayoutKind.Sequential)]
        private struct SecurityAttributes
        {
            public uint Length;
            public IntPtr SecurityDescriptor;
            [MarshalAs(UnmanagedType.Bool)] public bool InheritHandle;
        }

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct StartupInfo
        {
            public uint Size;
            public string Reserved;
            public string Desktop;
            public string Title;
            public uint X, Y, XSize, YSize, XCountChars, YCountChars, FillAttribute, Flags;
            public ushort ShowWindow, Reserved2;
            public IntPtr ReservedPointer, StdInput, StdOutput, StdError;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct StartupInfoEx
        {
            public StartupInfo Startup;
            public IntPtr AttributeList;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct ProcessInformation
        {
            public IntPtr Process, Thread;
            public uint ProcessId, ThreadId;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct BasicLimitInformation
        {
            public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
            public uint LimitFlags;
            public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
            public uint ActiveProcessLimit;
            public UIntPtr Affinity;
            public uint PriorityClass, SchedulingClass;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct IoCounters
        {
            public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount;
            public ulong ReadTransferCount, WriteTransferCount, OtherTransferCount;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct ExtendedLimitInformation
        {
            public BasicLimitInformation Basic;
            public IoCounters Io;
            public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CompletionPortInformation
        {
            public IntPtr CompletionKey, CompletionPort;
        }

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool SetInformationJobObject(IntPtr job, int infoClass, IntPtr information, uint length);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool QueryInformationJobObject(IntPtr job, int infoClass, out ExtendedLimitInformation information, uint length, IntPtr returnedLength);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool TerminateJobObject(IntPtr job, uint exitCode);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool TerminateProcess(IntPtr process, uint exitCode);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern uint ResumeThread(IntPtr thread);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CloseHandle(IntPtr handle);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr CreateFileW(string path, uint access, uint share, ref SecurityAttributes attributes, uint disposition, uint flags, IntPtr template);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr CreateIoCompletionPort(IntPtr file, IntPtr existingPort, UIntPtr key, uint threads);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool GetQueuedCompletionStatus(IntPtr port, out uint message, out UIntPtr key, out IntPtr overlapped, uint milliseconds);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, uint flags, ref IntPtr size);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, IntPtr attribute, IntPtr value, IntPtr size, IntPtr previous, IntPtr returned);
        [DllImport("kernel32.dll")]
        private static extern void DeleteProcThreadAttributeList(IntPtr list);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CreateProcessW(string application, StringBuilder commandLine, IntPtr processAttributes, IntPtr threadAttributes,
            [MarshalAs(UnmanagedType.Bool)] bool inheritHandles, uint creationFlags, IntPtr environment, string directory,
            ref StartupInfoEx startup, out ProcessInformation information);

        private static void Check(bool success, string operation)
        {
            if (!success) throw new Win32Exception(Marshal.GetLastWin32Error(), operation + " failed");
        }

        private static void SetJobInformation<T>(IntPtr job, int infoClass, T information) where T : struct
        {
            int size = Marshal.SizeOf<T>();
            IntPtr pointer = Marshal.AllocHGlobal(size);
            try
            {
                Marshal.StructureToPtr(information, pointer, false);
                Check(SetInformationJobObject(job, infoClass, pointer, (uint)size), "SetInformationJobObject");
            }
            finally { Marshal.FreeHGlobal(pointer); }
        }

        // MS CRT argv quoting: no shell is involved and every argument is quoted.
        private static string Quote(string value)
        {
            if (value == null || value.IndexOf('\0') >= 0) throw new ArgumentException("Arguments must be non-null strings without NUL");
            var result = new StringBuilder("\"");
            int slashes = 0;
            foreach (char item in value)
            {
                if (item == '\\') { slashes++; continue; }
                if (item == '"') { result.Append('\\', slashes * 2 + 1); result.Append('"'); }
                else { result.Append('\\', slashes); result.Append(item); }
                slashes = 0;
            }
            result.Append('\\', slashes * 2);
            result.Append('"');
            return result.ToString();
        }

        private static IntPtr EnvironmentBlock(IDictionary overrides)
        {
            var values = new SortedDictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            foreach (DictionaryEntry pair in Environment.GetEnvironmentVariables()) values[(string)pair.Key] = (string)pair.Value;
            if (overrides != null)
            {
                foreach (DictionaryEntry pair in overrides)
                {
                    string key = pair.Key as string;
                    if (String.IsNullOrEmpty(key) || key.IndexOfAny(new[] { '=', '\0' }) >= 0) throw new ArgumentException("Invalid environment variable name");
                    if (pair.Value == null) { values.Remove(key); continue; }
                    string value = pair.Value as string;
                    if (value == null || value.IndexOf('\0') >= 0) throw new ArgumentException("Environment values must be strings without NUL");
                    values[key] = value;
                }
            }
            var block = new StringBuilder();
            foreach (var pair in values) block.Append(pair.Key).Append('=').Append(pair.Value).Append('\0');
            block.Append('\0');
            return Marshal.StringToHGlobalUni(block.ToString());
        }

        private static IntPtr OpenOutput(string path, ref SecurityAttributes attributes)
        {
            // CREATE_NEW prevents overwriting prior logs and following an existing link.
            IntPtr handle = CreateFileW(path, 0x40000000, 1, ref attributes, 1, 0x80, IntPtr.Zero);
            if (handle == InvalidHandle) throw new Win32Exception(Marshal.GetLastWin32Error(), "Create new process output failed");
            return handle;
        }

        public static RunResult Run(string executable, string[] arguments, string directory, string stdoutPath, string stderrPath,
            ulong memoryLimitBytes, long timeoutMilliseconds, IDictionary environment, Action<uint> onAssigned = null, Action<uint> onRunning = null)
        {
            if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows)) throw new PlatformNotSupportedException("Windows Job Objects are required");
            if (memoryLimitBytes < 32UL * 1024 * 1024 || timeoutMilliseconds < 0) throw new ArgumentException("Invalid process resource limit");
            var result = new RunResult { Status = "starting", MemoryLimitBytes = memoryLimitBytes };
            IntPtr job = IntPtr.Zero, completion = IntPtr.Zero, stdout = IntPtr.Zero, stderr = IntPtr.Zero, stdin = IntPtr.Zero;
            IntPtr attributes = IntPtr.Zero, handleList = IntPtr.Zero, environmentBlock = IntPtr.Zero;
            bool attributeListInitialized = false, created = false, assigned = false;
            var process = new ProcessInformation();
            var clock = Stopwatch.StartNew();
            try
            {
                // Job handles must NOT be inheritable: closing/killing this runner must
                // close the last handle even when children still hold their stdout handles.
                job = CreateJobObjectW(IntPtr.Zero, null);
                Check(job != IntPtr.Zero, "CreateJobObject");
                SetJobInformation(job, 9, new ExtendedLimitInformation {
                    Basic = new BasicLimitInformation { LimitFlags = JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE },
                    JobMemoryLimit = new UIntPtr(memoryLimitBytes)
                });
                completion = CreateIoCompletionPort(InvalidHandle, IntPtr.Zero, UIntPtr.Zero, 1);
                Check(completion != IntPtr.Zero, "CreateIoCompletionPort");
                SetJobInformation(job, 7, new CompletionPortInformation { CompletionKey = new IntPtr(1), CompletionPort = completion });

                var security = new SecurityAttributes { Length = (uint)Marshal.SizeOf<SecurityAttributes>(), InheritHandle = true };
                stdout = OpenOutput(stdoutPath, ref security);
                stderr = OpenOutput(stderrPath, ref security);
                stdin = CreateFileW("NUL", 0x80000000, 3, ref security, 3, 0x80, IntPtr.Zero);
                Check(stdin != InvalidHandle, "Open stdin NUL");

                IntPtr attributeBytes = IntPtr.Zero;
                InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref attributeBytes);
                if (attributeBytes == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "Size process attributes failed");
                attributes = Marshal.AllocHGlobal(attributeBytes);
                Check(InitializeProcThreadAttributeList(attributes, 1, 0, ref attributeBytes), "Initialize process attributes");
                attributeListInitialized = true;
                handleList = Marshal.AllocHGlobal(IntPtr.Size * 3);
                Marshal.WriteIntPtr(handleList, 0, stdin);
                Marshal.WriteIntPtr(handleList, IntPtr.Size, stdout);
                Marshal.WriteIntPtr(handleList, IntPtr.Size * 2, stderr);
                Check(UpdateProcThreadAttribute(attributes, 0, new IntPtr(0x20002), handleList, new IntPtr(IntPtr.Size * 3), IntPtr.Zero, IntPtr.Zero), "Set inherited stdio handles");

                var startup = new StartupInfoEx {
                    Startup = new StartupInfo { Size = (uint)Marshal.SizeOf<StartupInfoEx>(), Flags = STARTF_USESTDHANDLES,
                        StdInput = stdin, StdOutput = stdout, StdError = stderr },
                    AttributeList = attributes
                };
                var commandLine = new StringBuilder(Quote(executable));
                foreach (string argument in arguments) commandLine.Append(' ').Append(Quote(argument));
                if (commandLine.Length >= 32767) throw new ArgumentException("Process command line is too long");
                environmentBlock = EnvironmentBlock(environment);
                Check(CreateProcessW(executable, commandLine, IntPtr.Zero, IntPtr.Zero, true,
                    CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
                    environmentBlock, directory, ref startup, out process), "Create suspended process");
                created = true;
                result.ProcessId = process.ProcessId;
                Check(AssignProcessToJobObject(job, process.Process), "Assign suspended process to bounded job");
                assigned = true;
                result.AssignedBeforeResume = true;
                if (onAssigned != null) onAssigned(process.ProcessId);
                Check(ResumeThread(process.Thread) != UInt32.MaxValue, "Resume bounded process");
                CloseHandle(process.Thread); process.Thread = IntPtr.Zero;
                result.Status = "exited";
                long nextObserverMilliseconds = clock.ElapsedMilliseconds;

                while (true)
                {
                    uint message; UIntPtr key; IntPtr overlapped;
                    bool notified = GetQueuedCompletionStatus(completion, out message, out key, out overlapped, 50);
                    if (notified && message == JOB_OBJECT_MSG_JOB_MEMORY_LIMIT)
                    {
                        result.Status = "memory-limit";
                        Check(TerminateJobObject(job, 125), "Terminate memory-limited job");
                        break;
                    }
                    if (!notified && Marshal.GetLastWin32Error() != (int)WAIT_TIMEOUT) Check(false, "Wait for job notification");
                    uint wait = WaitForSingleObject(process.Process, 0);
                    if (wait == WAIT_OBJECT_0) break;
                    if (wait != WAIT_TIMEOUT) Check(false, "Wait for bounded process");
                    if (timeoutMilliseconds > 0 && clock.ElapsedMilliseconds >= timeoutMilliseconds)
                    {
                        result.Status = "timeout";
                        Check(TerminateJobObject(job, 124), "Terminate timed-out job");
                        break;
                    }
                    // Trusted, synchronous observer; an exception still closes
                    // the only job handle in finally and kills all descendants.
                    if (onRunning != null && clock.ElapsedMilliseconds >= nextObserverMilliseconds)
                    {
                        onRunning(process.ProcessId);
                        nextObserverMilliseconds = clock.ElapsedMilliseconds + 1000;
                    }
                }
                Check(WaitForSingleObject(process.Process, 10000) == WAIT_OBJECT_0, "Confirm bounded process termination");
                uint exitCode;
                Check(GetExitCodeProcess(process.Process, out exitCode), "Read bounded process exit code");
                result.ProcessExitCode = exitCode;
                result.ExitCode = result.Status == "timeout" ? 124 : result.Status == "memory-limit" ? 125 : unchecked((int)exitCode);
                ExtendedLimitInformation usage;
                Check(QueryInformationJobObject(job, 9, out usage, (uint)Marshal.SizeOf<ExtendedLimitInformation>(), IntPtr.Zero), "Read job memory usage");
                if (usage.JobMemoryLimit.ToUInt64() != memoryLimitBytes ||
                    (usage.Basic.LimitFlags & (JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)) !=
                    (JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE))
                    throw new InvalidOperationException("Windows job limits changed unexpectedly");
                result.PeakJobMemoryBytes = usage.PeakJobMemoryUsed.ToUInt64();
                return result;
            }
            finally
            {
                // Covers failed assignment before resume and all exceptions afterwards.
                // Never addresses a process by a possibly reused PID.
                if (created && !assigned) TerminateProcess(process.Process, 126);
                if (job != IntPtr.Zero) { CloseHandle(job); job = IntPtr.Zero; }
                if (created) WaitForSingleObject(process.Process, 10000);
                foreach (IntPtr handle in new[] { process.Thread, process.Process, stdin, stdout, stderr, completion })
                    if (handle != IntPtr.Zero && handle != InvalidHandle) CloseHandle(handle);
                if (attributeListInitialized) DeleteProcThreadAttributeList(attributes);
                if (attributes != IntPtr.Zero) Marshal.FreeHGlobal(attributes);
                if (handleList != IntPtr.Zero) Marshal.FreeHGlobal(handleList);
                if (environmentBlock != IntPtr.Zero) Marshal.FreeHGlobal(environmentBlock);
                clock.Stop();
                result.ElapsedMilliseconds = clock.ElapsedMilliseconds;
            }
        }
    }
}
