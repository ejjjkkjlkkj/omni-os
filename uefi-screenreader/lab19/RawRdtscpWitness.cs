using System;
using System.Runtime.InteropServices;
using System.Runtime.Intrinsics.X86;
using System.Security.Cryptography;
using System.Threading.Tasks;

public static class Lab19Witness
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate ulong RdtscpDelegate(IntPtr auxOut);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr VirtualAlloc(IntPtr address, UIntPtr size, uint allocationType, uint protect);

    private static readonly byte[] MachineCode = new byte[] {
        0x49,0x89,0xC8,
        0x0F,0x01,0xF9,
        0x41,0x89,0x08,
        0x48,0xC1,0xE2,0x20,
        0x48,0x09,0xD0,
        0xC3
    };

    private static readonly RdtscpDelegate ReadCounter;

    private static string Hash(byte[] bytes)
    {
        using (var sha = SHA256.Create())
            return BitConverter.ToString(sha.ComputeHash(bytes)).Replace("-", "").ToLowerInvariant();
    }

    private static void RequireRdtscp()
    {
        if (!X86Base.IsSupported) throw new PlatformNotSupportedException("X86Base");
        var max = X86Base.CpuId(unchecked((int)0x80000000u), 0);
        if (unchecked((uint)max.Item1) < 0x80000001u)
            throw new PlatformNotSupportedException("extended CPUID leaf 0x80000001");
        var ext = X86Base.CpuId(unchecked((int)0x80000001u), 0);
        uint edx = unchecked((uint)ext.Item4);
        if ((edx & (1u << 27)) == 0)
            throw new PlatformNotSupportedException("RDTSCP");
    }

    static Lab19Witness()
    {
        const uint MEM_COMMIT = 0x1000;
        const uint MEM_RESERVE = 0x2000;
        const uint PAGE_EXECUTE_READWRITE = 0x40;

        RequireRdtscp();
        if (Hash(MachineCode) != "a11c5b2d56347072659406ba756714b2af33a9ac4d37536a86a0c1c8a6088a32")
            throw new Exception("RDTSCP machine-code hash mismatch");

        IntPtr p = VirtualAlloc(IntPtr.Zero, (UIntPtr)MachineCode.Length, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE);
        if (p == IntPtr.Zero)
            throw new InvalidOperationException("VirtualAlloc failed: " + Marshal.GetLastWin32Error());
        Marshal.Copy(MachineCode, 0, p, MachineCode.Length);
        ReadCounter = Marshal.GetDelegateForFunctionPointer<RdtscpDelegate>(p);
    }

    public static string Run(int workers, int samplesPerWorker)
    {
        if (workers <= 0 || samplesPerWorker <= 0)
            throw new ArgumentOutOfRangeException();

        var firstAux = new uint[workers];
        var lastAux = new uint[workers];

        Parallel.For(0, workers, worker =>
        {
            IntPtr aux = Marshal.AllocHGlobal(4);
            try
            {
                Marshal.WriteInt32(aux, 0);
                ulong first = ReadCounter(aux);
                ulong previous = first;
                firstAux[worker] = unchecked((uint)Marshal.ReadInt32(aux));
                ulong last = first;

                for (int i = 0; i < samplesPerWorker; i++)
                {
                    ulong current = ReadCounter(aux);
                    if (current < previous)
                        throw new Exception("raw RDTSCP moved backward on worker " + worker + " at sample " + i);
                    previous = current;
                    last = current;
                }

                lastAux[worker] = unchecked((uint)Marshal.ReadInt32(aux));
                if (last <= first)
                    throw new Exception("raw RDTSCP did not advance on worker " + worker);
            }
            finally
            {
                Marshal.FreeHGlobal(aux);
            }
        });

        return "LAB19_RAW_RDTSCP=PASS;WORKERS=" + workers
            + ";SAMPLES_PER_WORKER=" + samplesPerWorker
            + ";CODE_BYTES=" + MachineCode.Length
            + ";CODE_SHA256=" + Hash(MachineCode)
            + ";AUX_FIRST=" + string.Join(",", firstAux)
            + ";AUX_LAST=" + string.Join(",", lastAux);
    }
}
