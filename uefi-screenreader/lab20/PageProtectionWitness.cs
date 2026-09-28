using System;
using System.Runtime.InteropServices;
using System.Security.Cryptography;

public static class Lab20Witness
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

    [DllImport("kernel32.dll")]
    private static extern uint SetErrorMode(uint mode);

    private const uint MEM_COMMIT = 0x1000;
    private const uint MEM_RESERVE = 0x2000;
    private const uint MEM_RELEASE = 0x8000;
    private const uint PAGE_READWRITE = 0x04;
    private const uint PAGE_EXECUTE_READ = 0x20;
    private const uint SEM_FAILCRITICALERRORS = 0x0001;
    private const uint SEM_NOGPFAULTERRORBOX = 0x0002;
    private const uint SEM_NOOPENFILEERRORBOX = 0x8000;

    private static readonly byte[] MachineCode = new byte[] {
        0xB8,0x2A,0x00,0x00,0x00,
        0xC3
    };

    private static string Hash(byte[] bytes)
    {
        using (var sha = SHA256.Create())
            return BitConverter.ToString(sha.ComputeHash(bytes)).Replace("-", "").ToLowerInvariant();
    }

    private static IntPtr AllocateRw()
    {
        IntPtr p = VirtualAlloc(IntPtr.Zero, (UIntPtr)4096, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if (p == IntPtr.Zero)
            throw new InvalidOperationException("VirtualAlloc PAGE_READWRITE failed: " + Marshal.GetLastWin32Error());
        Marshal.Copy(MachineCode, 0, p, MachineCode.Length);
        return p;
    }

    private static void MakeRx(IntPtr p)
    {
        uint oldProtect;
        if (!VirtualProtect(p, (UIntPtr)4096, PAGE_EXECUTE_READ, out oldProtect))
            throw new InvalidOperationException("VirtualProtect PAGE_EXECUTE_READ failed: " + Marshal.GetLastWin32Error());
        if (oldProtect != PAGE_READWRITE)
            throw new Exception("unexpected previous page protection: 0x" + oldProtect.ToString("x"));
        if (!FlushInstructionCache(GetCurrentProcess(), p, (UIntPtr)MachineCode.Length))
            throw new InvalidOperationException("FlushInstructionCache failed: " + Marshal.GetLastWin32Error());
    }

    private static void Free(IntPtr p)
    {
        if (p != IntPtr.Zero)
            VirtualFree(p, UIntPtr.Zero, MEM_RELEASE);
    }

    public static string CodeSha256()
    {
        string hash = Hash(MachineCode);
        if (hash != "11db5348e275fb704be582e8005ee7d604f7f17b154d6cc644d240eef29d456a")
            throw new Exception("return-42 machine-code hash mismatch");
        return hash;
    }

    public static string ExecuteAfterRx()
    {
        CodeSha256();
        IntPtr p = AllocateRw();
        try
        {
            MakeRx(p);
            var fn = Marshal.GetDelegateForFunctionPointer<ReturnDelegate>(p);
            int value = fn();
            if (value != 42)
                throw new Exception("RX execution returned " + value + " instead of 42");
            return "LAB20_RX_EXECUTION=PASS;VALUE=" + value + ";CODE_SHA256=" + CodeSha256();
        }
        finally
        {
            Free(p);
        }
    }

    public static int ExecuteWhileRw()
    {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
        CodeSha256();
        IntPtr p = AllocateRw();
        try
        {
            var fn = Marshal.GetDelegateForFunctionPointer<ReturnDelegate>(p);
            return fn();
        }
        finally
        {
            Free(p);
        }
    }

    public static void WriteAfterRx()
    {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
        CodeSha256();
        IntPtr p = AllocateRw();
        try
        {
            MakeRx(p);
            Marshal.WriteByte(p, 0, 0x90);
        }
        finally
        {
            Free(p);
        }
    }
}
