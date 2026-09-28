using System;
using System.Runtime.InteropServices;
using System.Security.Cryptography;

public static class Lab23Witness
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int ReturnDelegate();

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr VirtualAlloc(IntPtr address, UIntPtr size, uint allocationType, uint protect);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool VirtualProtect(IntPtr address, UIntPtr size, uint newProtect, out uint oldProtect);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool VirtualFree(IntPtr address, UIntPtr size, uint freeType);

    [DllImport("kernel32.dll")]
    private static extern IntPtr GetCurrentProcess();

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool FlushInstructionCache(IntPtr process, IntPtr baseAddress, UIntPtr size);

    private const uint MEM_COMMIT = 0x1000;
    private const uint MEM_RESERVE = 0x2000;
    private const uint MEM_RELEASE = 0x8000;
    private const uint PAGE_READWRITE = 0x04;
    private const uint PAGE_EXECUTE_READ = 0x20;

    private static int ExpectedValue(int iteration)
    {
        uint value = unchecked(0x9E3779B9u * (uint)(iteration + 1));
        value ^= 0xA5A5A5A5u;
        return unchecked((int)value);
    }

    private static byte[] CodeFor(int value)
    {
        byte[] imm = BitConverter.GetBytes(value);
        return new byte[] { 0xB8, imm[0], imm[1], imm[2], imm[3], 0xC3 };
    }

    private static string Hash(byte[] data)
    {
        using (var sha = SHA256.Create())
            return BitConverter.ToString(sha.ComputeHash(data)).Replace("-", "").ToLowerInvariant();
    }

    public static string Run(int iterations)
    {
        if (iterations <= 0)
            throw new ArgumentOutOfRangeException("iterations");

        IntPtr page = VirtualAlloc(IntPtr.Zero, (UIntPtr)4096, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if (page == IntPtr.Zero)
            throw new InvalidOperationException("VirtualAlloc PAGE_READWRITE failed: " + Marshal.GetLastWin32Error());

        try
        {
            var fn = Marshal.GetDelegateForFunctionPointer<ReturnDelegate>(page);
            byte[] trace = new byte[checked(iterations * 4)];

            for (int i = 0; i < iterations; i++)
            {
                int expected = ExpectedValue(i);
                byte[] code = CodeFor(expected);
                Marshal.Copy(code, 0, page, code.Length);

                uint oldProtect;
                if (!VirtualProtect(page, (UIntPtr)4096, PAGE_EXECUTE_READ, out oldProtect))
                    throw new InvalidOperationException("VirtualProtect RW->RX failed at " + i + ": " + Marshal.GetLastWin32Error());
                if (oldProtect != PAGE_READWRITE)
                    throw new Exception("RW->RX previous protection mismatch at " + i + ": 0x" + oldProtect.ToString("x"));

                if (!FlushInstructionCache(GetCurrentProcess(), page, (UIntPtr)code.Length))
                    throw new InvalidOperationException("FlushInstructionCache failed at " + i + ": " + Marshal.GetLastWin32Error());

                int actual = fn();
                if (actual != expected)
                    throw new Exception("execution mismatch at " + i + ": actual=" + actual + " expected=" + expected);

                byte[] observed = BitConverter.GetBytes(actual);
                Buffer.BlockCopy(observed, 0, trace, i * 4, 4);

                if (!VirtualProtect(page, (UIntPtr)4096, PAGE_READWRITE, out oldProtect))
                    throw new InvalidOperationException("VirtualProtect RX->RW failed at " + i + ": " + Marshal.GetLastWin32Error());
                if (oldProtect != PAGE_EXECUTE_READ)
                    throw new Exception("RX->RW previous protection mismatch at " + i + ": 0x" + oldProtect.ToString("x"));
            }

            string traceHash = Hash(trace);
            if (iterations == 4096 && traceHash != "808f67bf865c85d399364767abdfc0ca96aa6ef2b32af55f5797f21fec9b405a")
                throw new Exception("trace hash mismatch: " + traceHash);

            return "LAB23_CODE_TRANSITIONS=PASS"
                + ";ITERATIONS=" + iterations
                + ";TRACE_BYTES=" + trace.Length
                + ";TRACE_SHA256=" + traceHash
                + ";FIRST_VALUE=" + unchecked((uint)ExpectedValue(0))
                + ";LAST_VALUE=" + unchecked((uint)ExpectedValue(iterations - 1))
                + ";RWX_USED=NO";
        }
        finally
        {
            VirtualFree(page, UIntPtr.Zero, MEM_RELEASE);
        }
    }
}
