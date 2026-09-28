using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Runtime.Intrinsics.X86;
using System.Threading;

public static class Lab22Witness
{
    [DllImport("kernel32.dll")]
    private static extern IntPtr GetCurrentThread();

    [DllImport("kernel32.dll")]
    private static extern IntPtr GetCurrentProcess();

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern UIntPtr SetThreadAffinityMask(IntPtr hThread, UIntPtr dwThreadAffinityMask);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetProcessAffinityMask(IntPtr hProcess, out UIntPtr processAffinityMask, out UIntPtr systemAffinityMask);

    [DllImport("kernel32.dll")]
    private static extern uint GetCurrentProcessorNumber();

    private sealed class Sample
    {
        public int Target;
        public uint ObservedProcessor;
        public byte LegacyApic;
        public uint ExtendedApic;
        public byte CoreId;
        public int ThreadsPerCore;
        public string Error;
    }

    private static uint U(int value)
    {
        return unchecked((uint)value);
    }

    private static ulong PtrToU64(UIntPtr value)
    {
        return UIntPtr.Size == 8 ? value.ToUInt64() : value.ToUInt32();
    }

    private static void RequireAmdTopology()
    {
        if (!X86Base.IsSupported)
            throw new PlatformNotSupportedException("X86Base");

        var basic = X86Base.CpuId(0, 0);
        if (U(basic.Item1) < 1u)
            throw new PlatformNotSupportedException("CPUID leaf 1");

        var maxExt = X86Base.CpuId(unchecked((int)0x80000000u), 0);
        if (U(maxExt.Item1) < 0x8000001Eu)
            throw new PlatformNotSupportedException("CPUID 0x8000001E");

        var extFeatures = X86Base.CpuId(unchecked((int)0x80000001u), 0);
        if ((U(extFeatures.Item3) & (1u << 22)) == 0)
            throw new PlatformNotSupportedException("AMD topology extensions");
    }

    private static void Capture(Sample sample, ManualResetEventSlim gate)
    {
        UIntPtr previous = UIntPtr.Zero;
        bool affinitySet = false;
        try
        {
            ulong mask = 1UL << sample.Target;
            previous = SetThreadAffinityMask(GetCurrentThread(), new UIntPtr(mask));
            if (previous == UIntPtr.Zero)
                throw new InvalidOperationException("SetThreadAffinityMask failed: " + Marshal.GetLastWin32Error());
            affinitySet = true;

            gate.Wait();

            uint observed = GetCurrentProcessorNumber();
            for (int spin = 0; observed != (uint)sample.Target && spin < 100000; spin++)
            {
                Thread.Yield();
                observed = GetCurrentProcessorNumber();
            }

            if (observed != (uint)sample.Target)
                throw new Exception("affinity target " + sample.Target + " observed processor " + observed);

            var leaf1 = X86Base.CpuId(1, 0);
            var topo = X86Base.CpuId(unchecked((int)0x8000001Eu), 0);

            sample.ObservedProcessor = observed;
            sample.LegacyApic = (byte)(U(leaf1.Item2) >> 24);
            sample.ExtendedApic = U(topo.Item1);
            sample.CoreId = (byte)(U(topo.Item2) & 0xffu);
            sample.ThreadsPerCore = checked((int)(((U(topo.Item2) >> 8) & 0xffu) + 1u));
        }
        catch (Exception ex)
        {
            sample.Error = ex.GetType().Name + ": " + ex.Message;
        }
        finally
        {
            if (affinitySet)
            {
                UIntPtr restored = SetThreadAffinityMask(GetCurrentThread(), previous);
                if (restored == UIntPtr.Zero && sample.Error == null)
                    sample.Error = "affinity restore failed: " + Marshal.GetLastWin32Error();
            }
        }
    }

    public static string Run(int logicalProcessors)
    {
        RequireAmdTopology();

        if (logicalProcessors <= 0 || logicalProcessors > 64)
            throw new ArgumentOutOfRangeException("logicalProcessors");
        if (Environment.ProcessorCount != logicalProcessors)
            throw new Exception("processor count mismatch: " + Environment.ProcessorCount);

        UIntPtr processMask;
        UIntPtr systemMask;
        if (!GetProcessAffinityMask(GetCurrentProcess(), out processMask, out systemMask))
            throw new InvalidOperationException("GetProcessAffinityMask failed: " + Marshal.GetLastWin32Error());

        ulong requiredMask = logicalProcessors == 64 ? ulong.MaxValue : ((1UL << logicalProcessors) - 1UL);
        ulong observedProcessMask = PtrToU64(processMask);
        ulong observedSystemMask = PtrToU64(systemMask);

        if ((observedProcessMask & requiredMask) != requiredMask)
            throw new Exception("process affinity mask does not expose all expected processors: 0x" + observedProcessMask.ToString("x"));
        if ((observedSystemMask & requiredMask) != requiredMask)
            throw new Exception("system affinity mask does not expose all expected processors: 0x" + observedSystemMask.ToString("x"));

        var samples = new Sample[logicalProcessors];
        var threads = new Thread[logicalProcessors];

        using (var gate = new ManualResetEventSlim(false))
        {
            for (int target = 0; target < logicalProcessors; target++)
            {
                int captured = target;
                samples[target] = new Sample { Target = target };
                threads[target] = new Thread(() => Capture(samples[captured], gate));
                threads[target].IsBackground = true;
                threads[target].Name = "lab22-affinity-" + target;
                threads[target].Start();
            }

            gate.Set();

            for (int i = 0; i < threads.Length; i++)
                threads[i].Join();
        }

        var legacy = new HashSet<byte>();
        var extended = new HashSet<uint>();
        var mappings = new List<string>(logicalProcessors);

        for (int i = 0; i < samples.Length; i++)
        {
            Sample s = samples[i];
            if (s.Error != null)
                throw new Exception("target " + i + " failed: " + s.Error);
            if (s.ObservedProcessor != (uint)i)
                throw new Exception("target/processor mismatch " + i + "/" + s.ObservedProcessor);
            if (!legacy.Add(s.LegacyApic))
                throw new Exception("duplicate legacy APIC ID " + s.LegacyApic);
            if (!extended.Add(s.ExtendedApic))
                throw new Exception("duplicate extended APIC ID " + s.ExtendedApic);
            if (s.ThreadsPerCore <= 0)
                throw new Exception("invalid threads-per-core target " + i);

            mappings.Add(
                s.Target
                + ":cpu=" + s.ObservedProcessor
                + ",apic8=" + s.LegacyApic
                + ",apic32=" + s.ExtendedApic
                + ",core=" + s.CoreId
                + ",tpc=" + s.ThreadsPerCore);
        }

        if (legacy.Count != logicalProcessors || extended.Count != logicalProcessors)
            throw new Exception("unique topology identity count mismatch");

        return "LAB22_AFFINITY_TOPOLOGY=PASS"
            + ";LOGICAL_PROCESSORS=" + logicalProcessors
            + ";PROCESS_MASK=0x" + observedProcessMask.ToString("x")
            + ";SYSTEM_MASK=0x" + observedSystemMask.ToString("x")
            + ";UNIQUE_APIC8=" + legacy.Count
            + ";UNIQUE_APIC32=" + extended.Count
            + ";MAP=" + string.Join("|", mappings);
    }
}
