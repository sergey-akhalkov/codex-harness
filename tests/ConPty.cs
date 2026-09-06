// Headless Windows terminal fixture. No process is launched in a visible window.
// API contract: https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;
using Microsoft.Win32.SafeHandles;

namespace Harness.Tests {
    public sealed class ConPty : IDisposable {
        [StructLayout(LayoutKind.Sequential)] struct Coord { public short X, Y; }
        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct StartupInfo {
            public int cb; public IntPtr reserved, desktop, title;
            public int x, y, xSize, ySize, xChars, yChars, fill, flags;
            public short show, reservedSize;
            public IntPtr reservedBytes, stdin, stdout, stderr;
        }
        [StructLayout(LayoutKind.Sequential)] struct StartupInfoEx { public StartupInfo Info; public IntPtr Attributes; }
        [StructLayout(LayoutKind.Sequential)] struct ProcessInfo { public IntPtr Process, Thread; public int Pid, Tid; }
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool CreatePipe(out IntPtr read, out IntPtr write, IntPtr attributes, int size);
        [DllImport("kernel32.dll")] static extern int CreatePseudoConsole(Coord size, IntPtr input, IntPtr output, int flags, out IntPtr console);
        [DllImport("kernel32.dll")] static extern void ClosePseudoConsole(IntPtr console);
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, int flags, ref IntPtr size);
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, IntPtr attribute, IntPtr value, IntPtr size, IntPtr old, IntPtr returned);
        [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool CreateProcessW(string executable, StringBuilder commandLine, IntPtr processAttributes, IntPtr threadAttributes, bool inherit, uint flags, IntPtr environment, string directory, ref StartupInfoEx startup, out ProcessInfo info);
        [DllImport("kernel32.dll", SetLastError = true)] static extern bool CloseHandle(IntPtr handle);
        [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
        [DllImport("kernel32.dll")] static extern bool GetExitCodeProcess(IntPtr process, out uint code);
        [DllImport("kernel32.dll")] static extern bool TerminateProcess(IntPtr process, uint code);
        readonly object gate = new object();
        readonly StringBuilder transcript = new StringBuilder();
        FileStream input, output;
        Task pump;
        IntPtr console, process;
        public int ProcessId { get; private set; }
        public string Transcript { get { lock (gate) return transcript.ToString(); } }
        public bool HasExited { get { return process != IntPtr.Zero && WaitForSingleObject(process, 0) == 0; } }
        public uint ExitCode { get { uint code; GetExitCodeProcess(process, out code); return code; } }

        public ConPty(string executable, string commandLine, string directory) {
            IntPtr inputRead = IntPtr.Zero, inputWrite = IntPtr.Zero, outputRead = IntPtr.Zero, outputWrite = IntPtr.Zero;
            IntPtr attributes = IntPtr.Zero;
            try {
                if (!CreatePipe(out inputRead, out inputWrite, IntPtr.Zero, 0) || !CreatePipe(out outputRead, out outputWrite, IntPtr.Zero, 0)) throw new Win32Exception(Marshal.GetLastWin32Error(), "CreatePipe");
                int result = CreatePseudoConsole(new Coord { X = 140, Y = 40 }, inputRead, outputWrite, 0, out console);
                if (result != 0) throw new COMException("CreatePseudoConsole failed", result);
                CloseHandle(inputRead); inputRead = IntPtr.Zero;
                CloseHandle(outputWrite); outputWrite = IntPtr.Zero;
                input = new FileStream(new SafeFileHandle(inputWrite, true), FileAccess.Write); inputWrite = IntPtr.Zero;
                output = new FileStream(new SafeFileHandle(outputRead, true), FileAccess.Read); outputRead = IntPtr.Zero;
                pump = Task.Run(() => {
                    var reader = new StreamReader(output, new UTF8Encoding(false), false, 4096, true);
                    char[] buffer = new char[4096];
                    try {
                        int length;
                        while ((length = reader.Read(buffer, 0, buffer.Length)) > 0) {
                            string fragment = new string(buffer, 0, length);
                            lock (gate) transcript.Append(fragment);
                            if (fragment.Contains("\x1b[6n")) Send("\x1b[1;1R");
                            if (fragment.Contains("\x1b[c")) Send("\x1b[?1;2c");
                        }
                    } catch (IOException) {} catch (ObjectDisposedException) {}
                });
                IntPtr size = IntPtr.Zero;
                InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref size);
                attributes = Marshal.AllocHGlobal(size);
                if (!InitializeProcThreadAttributeList(attributes, 1, 0, ref size)) throw new Win32Exception(Marshal.GetLastWin32Error(), "InitializeProcThreadAttributeList");
                if (!UpdateProcThreadAttribute(attributes, 0, (IntPtr)0x00020016, console, (IntPtr)IntPtr.Size, IntPtr.Zero, IntPtr.Zero)) throw new Win32Exception(Marshal.GetLastWin32Error(), "UpdateProcThreadAttribute");
                var startup = new StartupInfoEx();
                startup.Info.cb = Marshal.SizeOf<StartupInfoEx>();
                // Our test runner itself has redirected streams. Null explicit
                // handles force the client onto ConPTY rather than inheriting
                // those redirects: https://github.com/microsoft/terminal/discussions/15814
                startup.Info.flags = 0x00000100; // STARTF_USESTDHANDLES
                startup.Attributes = attributes;
                ProcessInfo info;
                if (!CreateProcessW(executable, new StringBuilder(commandLine), IntPtr.Zero, IntPtr.Zero, false, 0x00080000, IntPtr.Zero, directory, ref startup, out info)) throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateProcessW");
                process = info.Process; ProcessId = info.Pid; CloseHandle(info.Thread);
            } catch { Dispose(); throw; }
            finally {
                if (attributes != IntPtr.Zero) { DeleteProcThreadAttributeList(attributes); Marshal.FreeHGlobal(attributes); }
                foreach (IntPtr handle in new[] {inputRead, inputWrite, outputRead, outputWrite}) if (handle != IntPtr.Zero) CloseHandle(handle);
            }
        }
        public void Send(string text) {
            byte[] bytes = Encoding.UTF8.GetBytes(text);
            lock (gate) { input.Write(bytes, 0, bytes.Length); input.Flush(); }
        }
        public bool Wait(int milliseconds) { return WaitForSingleObject(process, (uint)milliseconds) == 0; }
        public void Dispose() {
            if (process != IntPtr.Zero && !HasExited) TerminateProcess(process, 1);
            if (console != IntPtr.Zero) { ClosePseudoConsole(console); console = IntPtr.Zero; }
            input?.Dispose();
            if (pump != null) pump.Wait(5000);
            output?.Dispose();
            if (process != IntPtr.Zero) { CloseHandle(process); process = IntPtr.Zero; }
        }
    }
}
