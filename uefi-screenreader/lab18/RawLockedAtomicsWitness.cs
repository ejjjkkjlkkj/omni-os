using System;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Threading.Tasks;

public static class Lab18Witness
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate long AtomicAddDelegate(IntPtr cell, long delta);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate long CompareExchangeDelegate(IntPtr cell, long expected, long desired);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr VirtualAlloc(IntPtr address, UIntPtr size, uint allocationType, uint protect);

    private static readonly byte[] XaddCode = new byte[] {
        0x48,0x89,0xD0,
        0xF0,0x48,0x0F,0xC1,0x01,
        0x48,0x01,0xD0,
        0xC3
    };

    private static readonly byte[] CasCode = new byte[] {
        0x48,0x89,0xD0,
        0xF0,0x4C,0x0F,0xB1,0x01,
        0xC3
    };

    private static readonly AtomicAddDelegate AtomicAdd;
    private static readonly CompareExchangeDelegate CompareExchange;

    private static IntPtr Install(byte[] code)
    {
        const uint MEM_COMMIT = 0x1000;
        const uint MEM_RESERVE = 0x2000;
        const uint PAGE_EXECUTE_READWRITE = 0x40;
        IntPtr p = VirtualAlloc(IntPtr.Zero, (UIntPtr)code.Length, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE);
        if (p == IntPtr.Zero)
            throw new InvalidOperationException("VirtualAlloc failed: " + Marshal.GetLastWin32Error());
        Marshal.Copy(code, 0, p, code.Length);
        return p;
    }

    private static string Hash(byte[] bytes)
    {
        using (var sha = SHA256.Create())
            return BitConverter.ToString(sha.ComputeHash(bytes)).Replace("-", "").ToLowerInvariant();
    }

    static Lab18Witness()
    {
        if (Hash(XaddCode) != "2b0d6ebb2de117485ed02d8dc3f59a1eb944844721f6e0ca2ef37eb119cfa581")
            throw new Exception("XADD machine-code hash mismatch");
        if (Hash(CasCode) != "7ba18e8f47c043338600423094c582078caebf65aaf6f75d15f3ff3a0918739f")
            throw new Exception("CAS machine-code hash mismatch");

        AtomicAdd = Marshal.GetDelegateForFunctionPointer<AtomicAddDelegate>(Install(XaddCode));
        CompareExchange = Marshal.GetDelegateForFunctionPointer<CompareExchangeDelegate>(Install(CasCode));
    }

    public static string Run(int workers, int incrementsPerWorker, int casRounds)
    {
        if (workers <= 0 || incrementsPerWorker <= 0 || casRounds <= 0)
            throw new ArgumentOutOfRangeException();

        IntPtr cell = Marshal.AllocHGlobal(8);
        try
        {
            Marshal.WriteInt64(cell, 0L);
            var observations = new long[workers][];

            Parallel.For(0, workers, worker =>
            {
                var local = new long[incrementsPerWorker];
                for (int i = 0; i < incrementsPerWorker; i++)
                    local[i] = AtomicAdd(cell, 1L);
                observations[worker] = local;
            });

            int expected = checked(workers * incrementsPerWorker);
            long finalCounter = Marshal.ReadInt64(cell);
            if (finalCounter != expected)
                throw new Exception("raw LOCK XADD final counter mismatch: " + finalCounter + " != " + expected);

            var seen = new bool[expected + 1];
            int unique = 0;
            for (int worker = 0; worker < workers; worker++)
            {
                var local = observations[worker];
                if (local == null || local.Length != incrementsPerWorker)
                    throw new Exception("missing XADD observation set for worker " + worker);

                for (int i = 0; i < local.Length; i++)
                {
                    long value = local[i];
                    if (value < 1 || value > expected)
                        throw new Exception("XADD return out of range: " + value);
                    int index = checked((int)value);
                    if (seen[index])
                        throw new Exception("duplicate XADD return value: " + value);
                    seen[index] = true;
                    unique++;
                }
            }

            if (unique != expected)
                throw new Exception("XADD unique return count mismatch: " + unique + " != " + expected);

            for (int round = 0; round < casRounds; round++)
            {
                Marshal.WriteInt64(cell, 0L);
                var observed = new long[workers];

                Parallel.For(0, workers, worker =>
                {
                    observed[worker] = CompareExchange(cell, 0L, worker + 1L);
                });

                int winners = 0;
                for (int worker = 0; worker < workers; worker++)
                    if (observed[worker] == 0L) winners++;

                long finalCas = Marshal.ReadInt64(cell);
                if (winners != 1)
                    throw new Exception("raw LOCK CMPXCHG winner mismatch at round " + round + ": " + winners);
                if (finalCas < 1 || finalCas > workers)
                    throw new Exception("raw LOCK CMPXCHG final state out of range at round " + round + ": " + finalCas);
            }

            return "LAB18_RAW_LOCKED_ATOMICS=PASS;WORKERS=" + workers
                + ";INCREMENTS_PER_WORKER=" + incrementsPerWorker
                + ";FINAL_COUNTER=" + finalCounter
                + ";UNIQUE_XADD_RETURNS=" + unique
                + ";CAS_ROUNDS=" + casRounds
                + ";XADD_CODE_SHA256=" + Hash(XaddCode)
                + ";CAS_CODE_SHA256=" + Hash(CasCode);
        }
        finally
        {
            Marshal.FreeHGlobal(cell);
        }
    }
}
