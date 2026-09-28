using System;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Threading.Tasks;

public static class Lab12Witness
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void ExchangeDelegate(IntPtr data, UIntPtr length);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr VirtualAlloc(IntPtr address, UIntPtr size, uint allocationType, uint protect);

    private static readonly byte[] MachineCode = new byte[] {
        0x48,0x85,0xD2,
        0x74,0x0B,
        0x80,0x31,0xFF,
        0x48,0xFF,0xC1,
        0x48,0xFF,0xCA,
        0x75,0xF5,
        0xC3
    };

    private static readonly IntPtr CodeAddress;
    private static readonly ExchangeDelegate Exchange;

    static Lab12Witness()
    {
        const uint MEM_COMMIT = 0x1000;
        const uint MEM_RESERVE = 0x2000;
        const uint PAGE_EXECUTE_READWRITE = 0x40;

        CodeAddress = VirtualAlloc(IntPtr.Zero, (UIntPtr)MachineCode.Length, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE);
        if (CodeAddress == IntPtr.Zero)
            throw new InvalidOperationException("VirtualAlloc failed: " + Marshal.GetLastWin32Error());

        Marshal.Copy(MachineCode, 0, CodeAddress, MachineCode.Length);
        Exchange = Marshal.GetDelegateForFunctionPointer<ExchangeDelegate>(CodeAddress);
    }

    private static byte[] Apply(byte[] input)
    {
        var data = (byte[])input.Clone();
        var handle = GCHandle.Alloc(data, GCHandleType.Pinned);
        try
        {
            Exchange(handle.AddrOfPinnedObject(), (UIntPtr)data.Length);
            return data;
        }
        finally
        {
            handle.Free();
        }
    }

    private static byte[] ExpectedExchange(byte[] src)
    {
        var dst = new byte[src.Length];
        for (int i = 0; i < src.Length; i++)
        {
            if (src[i] == 0) dst[i] = 255;
            else if (src[i] == 255) dst[i] = 0;
            else throw new Exception("unexpected external state");
        }
        return dst;
    }

    private static bool Equal(byte[] a, byte[] b)
    {
        if (a.Length != b.Length) return false;
        for (int i = 0; i < a.Length; i++) if (a[i] != b[i]) return false;
        return true;
    }

    private static byte[] Grow(byte[] material)
    {
        var opposite = ExpectedExchange(material);
        var next = new byte[material.Length * 2];
        Array.Copy(material, 0, next, 0, material.Length);
        Array.Copy(opposite, 0, next, material.Length, material.Length);
        return next;
    }

    private static void ValidateOnce()
    {
        byte[] material = new byte[] { 0, 255 };
        int[] widths = new[] { 2,4,8,16,32,64,128,256 };

        for (int stage = 0; stage < widths.Length; stage++)
        {
            int width = widths[stage];
            if (material.Length != width) throw new Exception("width mismatch");

            var expected = ExpectedExchange(material);
            var actual = Apply(material);
            if (!Equal(actual, expected)) throw new Exception("raw exchange mismatch at width " + width);

            var roundTrip = Apply(actual);
            if (!Equal(roundTrip, material)) throw new Exception("raw exchange involution mismatch at width " + width);

            if (stage + 1 < widths.Length)
                material = Grow(material);
        }
    }

    public static string CodeSha256()
    {
        using (var sha = SHA256.Create())
            return BitConverter.ToString(sha.ComputeHash(MachineCode)).Replace("-", "").ToLowerInvariant();
    }

    public static string Run(int workers, int repeatsPerWorker)
    {
        const string expectedCodeHash = "6fc989173743c7a8365b35253d17c746da914c15aff357252d9a139557e5edd5";
        if (CodeSha256() != expectedCodeHash) throw new Exception("raw machine-code hash mismatch");

        ValidateOnce();

        Parallel.For(0, workers, _ =>
        {
            for (int i = 0; i < repeatsPerWorker; i++)
                ValidateOnce();
        });

        return "LAB12_RAW_EXECUTION=PASS;CODE_BYTES=17;WORKERS=" + workers + ";REPEATS_PER_WORKER=" + repeatsPerWorker;
    }
}
