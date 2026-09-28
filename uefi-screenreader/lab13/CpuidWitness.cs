using System;
using System.Collections.Generic;
using System.Collections.Concurrent;
using System.Runtime.Intrinsics.X86;
using System.Text;
using System.Threading.Tasks;

public static class Lab13Witness
{
    private static void Append(List<byte> bytes, int value)
    {
        bytes.AddRange(BitConverter.GetBytes(value));
    }

    public static string Vendor()
    {
        if (!X86Base.IsSupported) throw new PlatformNotSupportedException("X86Base");
        var r = X86Base.CpuId(0, 0);
        var bytes = new List<byte>(12);
        Append(bytes, r.Item2);
        Append(bytes, r.Item4);
        Append(bytes, r.Item3);
        return Encoding.ASCII.GetString(bytes.ToArray());
    }

    public static string Brand()
    {
        if (!X86Base.IsSupported) throw new PlatformNotSupportedException("X86Base");
        var max = X86Base.CpuId(unchecked((int)0x80000000u), 0);
        if (unchecked((uint)max.Item1) < 0x80000004u) return "";
        var bytes = new List<byte>(48);
        for (uint leaf = 0x80000002u; leaf <= 0x80000004u; leaf++)
        {
            var r = X86Base.CpuId(unchecked((int)leaf), 0);
            Append(bytes, r.Item1);
            Append(bytes, r.Item2);
            Append(bytes, r.Item3);
            Append(bytes, r.Item4);
        }
        return Encoding.ASCII.GetString(bytes.ToArray()).Trim('\0', ' ');
    }

    public static string StableSnapshot()
    {
        var zero = X86Base.CpuId(0, 0);
        var one = X86Base.CpuId(1, 0);
        var ext = X86Base.CpuId(unchecked((int)0x80000000u), 0);
        uint leaf1EbxStable = unchecked((uint)one.Item2) & 0x00ffffffu;
        return "VENDOR=" + Vendor()
            + ";BRAND=" + Brand()
            + ";MAX_BASIC=0x" + unchecked((uint)zero.Item1).ToString("x8")
            + ";MAX_EXT=0x" + unchecked((uint)ext.Item1).ToString("x8")
            + ";LEAF1_EAX=0x" + unchecked((uint)one.Item1).ToString("x8")
            + ";LEAF1_EBX_LOW24=0x" + leaf1EbxStable.ToString("x6")
            + ";LEAF1_ECX=0x" + unchecked((uint)one.Item3).ToString("x8")
            + ";LEAF1_EDX=0x" + unchecked((uint)one.Item4).ToString("x8");
    }

    public static byte ApicId()
    {
        var one = X86Base.CpuId(1, 0);
        return (byte)(unchecked((uint)one.Item2) >> 24);
    }

    public static string Run(int workers, int repeatsPerWorker)
    {
        if (!X86Base.IsSupported) throw new PlatformNotSupportedException("X86Base");
        string vendor = Vendor();
        string brand = Brand();
        if (vendor != "AuthenticAMD") throw new Exception("unexpected vendor: " + vendor);
        if (brand.IndexOf("5800H", StringComparison.OrdinalIgnoreCase) < 0) throw new Exception("unexpected brand: " + brand);

        string reference = StableSnapshot();
        var apicIds = new ConcurrentDictionary<byte, byte>();

        Parallel.For(0, workers, _ =>
        {
            for (int i = 0; i < repeatsPerWorker; i++)
            {
                if (StableSnapshot() != reference) throw new Exception("stable CPUID identity changed during run");
                byte apic = ApicId();
                apicIds.TryAdd(apic, apic);
            }
        });

        var ids = new List<byte>(apicIds.Keys);
        ids.Sort();
        return "LAB13_CPUID=PASS;WORKERS=" + workers
            + ";REPEATS_PER_WORKER=" + repeatsPerWorker
            + ";OBSERVED_APIC_IDS=" + string.Join(",", ids)
            + ";" + reference;
    }
}
