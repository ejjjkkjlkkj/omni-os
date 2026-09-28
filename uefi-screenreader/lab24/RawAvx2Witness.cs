using System;
using System.Runtime.InteropServices;
using System.Runtime.Intrinsics.X86;
using System.Security.Cryptography;
using System.Threading.Tasks;

public static class Lab24Witness
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void Xor256Delegate(IntPtr destination, IntPtr source);

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

    private static readonly byte[] MachineCode = new byte[] {
        0xC5,0xFE,0x6F,0x01,
        0xC5,0xFD,0xEF,0x02,
        0xC5,0xFE,0x7F,0x01,
        0xC5,0xF8,0x77,
        0xC3
    };

    private static readonly IntPtr CodeAddress;
    private static readonly Xor256Delegate Xor256;

    private static string Hash(byte[] data)
    {
        using (var sha = SHA256.Create())
            return BitConverter.ToString(sha.ComputeHash(data)).Replace("-", "").ToLowerInvariant();
    }

    static Lab24Witness()
    {
        if (!Avx.IsSupported || !Avx2.IsSupported)
            throw new PlatformNotSupportedException("AVX2 OS/runtime support");

        string codeHash = Hash(MachineCode);
        if (codeHash != "4b54de4a087c29647967d35477ba3fc1ba416d4f114c298f89be4e31f69ea032")
            throw new Exception("AVX2 machine-code hash mismatch: " + codeHash);

        IntPtr p = VirtualAlloc(IntPtr.Zero, (UIntPtr)4096, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if (p == IntPtr.Zero)
            throw new InvalidOperationException("VirtualAlloc failed: " + Marshal.GetLastWin32Error());

        try
        {
            Marshal.Copy(MachineCode, 0, p, MachineCode.Length);
            uint oldProtect;
            if (!VirtualProtect(p, (UIntPtr)4096, PAGE_EXECUTE_READ, out oldProtect))
                throw new InvalidOperationException("VirtualProtect RW->RX failed: " + Marshal.GetLastWin32Error());
            if (oldProtect != PAGE_READWRITE)
                throw new Exception("unexpected prior protection: 0x" + oldProtect.ToString("x"));
            if (!FlushInstructionCache(GetCurrentProcess(), p, (UIntPtr)MachineCode.Length))
                throw new InvalidOperationException("FlushInstructionCache failed: " + Marshal.GetLastWin32Error());

            CodeAddress = p;
            Xor256 = Marshal.GetDelegateForFunctionPointer<Xor256Delegate>(CodeAddress);
        }
        catch
        {
            VirtualFree(p, UIntPtr.Zero, MEM_RELEASE);
            throw;
        }
    }

    private static void ValidateWorker(int worker, int repeats)
    {
        byte[] source = new byte[32];
        byte[] destination = new byte[32];
        byte[] original = new byte[32];
        byte[] expected = new byte[32];

        for (int i = 0; i < 32; i++)
        {
            source[i] = (byte)((worker * 17 + i * 29 + 0x5A) & 0xff);
            destination[i] = (byte)((worker * 31 + i * 7 + 0xA5) & 0xff);
            original[i] = destination[i];
            expected[i] = (byte)(destination[i] ^ source[i]);
        }

        var srcHandle = GCHandle.Alloc(source, GCHandleType.Pinned);
        var dstHandle = GCHandle.Alloc(destination, GCHandleType.Pinned);
        try
        {
            IntPtr src = srcHandle.AddrOfPinnedObject();
            IntPtr dst = dstHandle.AddrOfPinnedObject();

            for (int r = 0; r < repeats; r++)
            {
                Xor256(dst, src);
                for (int i = 0; i < 32; i++)
                    if (destination[i] != expected[i])
                        throw new Exception("AVX2 XOR mismatch worker=" + worker + " repeat=" + r + " byte=" + i);

                Xor256(dst, src);
                for (int i = 0; i < 32; i++)
                    if (destination[i] != original[i])
                        throw new Exception("AVX2 XOR roundtrip mismatch worker=" + worker + " repeat=" + r + " byte=" + i);
            }
        }
        finally
        {
            dstHandle.Free();
            srcHandle.Free();
        }
    }

    public static string Run(int workers, int repeatsPerWorker)
    {
        if (workers <= 0 || repeatsPerWorker <= 0)
            throw new ArgumentOutOfRangeException();

        Parallel.For(0, workers, worker => ValidateWorker(worker, repeatsPerWorker));

        return "LAB24_RAW_AVX2=PASS"
            + ";WORKERS=" + workers
            + ";REPEATS_PER_WORKER=" + repeatsPerWorker
            + ";CALLS=" + checked(workers * repeatsPerWorker * 2)
            + ";VECTOR_BYTES=32"
            + ";CODE_BYTES=" + MachineCode.Length
            + ";CODE_SHA256=" + Hash(MachineCode)
            + ";RWX_USED=NO";
    }
}
