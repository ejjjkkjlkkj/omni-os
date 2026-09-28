using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Threading.Tasks;

public static class NativeAllocatorV1
{
    private const int KeyWidth = 8;
    private const int MapRecordWidth = 16;
    private const int StateRecordWidth = 9;

    private static byte[] States(byte[] table)
    {
        var unique = new List<byte>();
        foreach (byte value in table)
            if (!unique.Contains(value)) unique.Add(value);
        if (unique.Count != 2) throw new Exception("expected exactly two native relation carriers");
        return unique.ToArray();
    }

    private static int IndexOf(byte[] states, byte value)
    {
        for (int i = 0; i < states.Length; i++)
            if (states[i] == value) return i;
        throw new Exception("value outside native relation carrier");
    }

    private static byte Compose(byte[] table, byte[] states, byte left, byte right)
    {
        return table[(IndexOf(states, left) * 2) + IndexOf(states, right)];
    }

    private static byte Identity(byte[] table, byte[] states)
    {
        var candidates = new List<byte>();
        foreach (byte candidate in states)
        {
            bool ok = true;
            foreach (byte x in states)
                if (Compose(table, states, candidate, x) != x ||
                    Compose(table, states, x, candidate) != x)
                {
                    ok = false;
                    break;
                }
            if (ok) candidates.Add(candidate);
        }
        if (candidates.Count != 1) throw new Exception("structural identity must be unique");
        return candidates[0];
    }

    private static bool KeyEquals(byte[] a, int ao, byte[] b, int bo)
    {
        for (int i = 0; i < KeyWidth; i++)
            if (a[ao + i] != b[bo + i]) return false;
        return true;
    }

    private static int Resolve(byte[] locations, byte[] key, int keyOffset)
    {
        int matches = 0;
        int slot = -1;
        for (int candidate = 0; candidate < 256; candidate++)
            if (KeyEquals(locations, candidate * KeyWidth, key, keyOffset))
            {
                matches++;
                slot = candidate;
            }
        if (matches != 1) throw new Exception("derived allocation target must resolve exactly once");
        return slot;
    }

    private static byte[] DomainMask(byte identity, byte exchange)
    {
        var mask = new byte[KeyWidth];
        for (int i = 0; i < KeyWidth; i++)
            mask[i] = (i % 2 == 0) ? identity : exchange;
        return mask;
    }

    private static void DeriveTarget(
        byte[] table, byte[] states, byte[] selector, int selectorOffset,
        byte[] mask, byte[] target)
    {
        for (int i = 0; i < KeyWidth; i++)
            target[i] = Compose(table, states, selector[selectorOffset + i], mask[i]);
    }

    public static string Exercise(
        string tablePath,
        string locationsPath,
        string allocationMapPath,
        string finalStatePath,
        string witnessPath,
        int workers)
    {
        if (workers < 2) throw new ArgumentOutOfRangeException("workers");

        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("LAB21 relation table length mismatch");
        byte[] states = States(table);
        byte available = Identity(table, states);
        byte held = states[0] == available ? states[1] : states[0];

        byte[] locations = File.ReadAllBytes(locationsPath);
        if (locations.Length != 256 * KeyWidth)
            throw new Exception("native location artifact length mismatch");

        byte[] mask = DomainMask(available, held);
        var targetBySelector = new byte[256 * KeyWidth];
        var targetSlotBySelector = new int[256];
        var seenTargets = new bool[256];

        for (int selectorOrdinal = 0; selectorOrdinal < 256; selectorOrdinal++)
        {
            int selectorOffset = selectorOrdinal * KeyWidth;
            var target = new byte[KeyWidth];
            DeriveTarget(table, states, locations, selectorOffset, mask, target);
            int slot = Resolve(locations, target, 0);
            if (seenTargets[slot]) throw new Exception("allocation target derivation is not bijective");
            seenTargets[slot] = true;
            targetSlotBySelector[selectorOrdinal] = slot;
            Array.Copy(target, 0, targetBySelector, selectorOffset, KeyWidth);
        }
        for (int i = 0; i < seenTargets.Length; i++)
            if (!seenTargets[i]) throw new Exception("allocation target derivation did not cover full pool");

        var cells = new int[256];
        for (int i = 0; i < cells.Length; i++) cells[i] = available;

        int firstTransitions = 0;
        Parallel.For(0, 256, new ParallelOptions { MaxDegreeOfParallelism = workers }, selectorOrdinal =>
        {
            int slot = targetSlotBySelector[selectorOrdinal];
            int observed = Interlocked.CompareExchange(ref cells[slot], held, available);
            if (observed != available) throw new Exception("first allocation failed on unique target");
            Interlocked.Increment(ref firstTransitions);
        });
        if (firstTransitions != 256) throw new Exception("first allocation transition count mismatch");

        int exhaustedRejections = 0;
        Parallel.For(0, 256, new ParallelOptions { MaxDegreeOfParallelism = workers }, selectorOrdinal =>
        {
            int slot = targetSlotBySelector[selectorOrdinal];
            int observed = Interlocked.CompareExchange(ref cells[slot], held, available);
            if (observed != held) throw new Exception("exhausted pool unexpectedly allocated target");
            Interlocked.Increment(ref exhaustedRejections);
        });
        if (exhaustedRejections != 256) throw new Exception("pool exhaustion rejection count mismatch");

        int firstReleases = 0;
        Parallel.For(0, 256, new ParallelOptions { MaxDegreeOfParallelism = workers }, selectorOrdinal =>
        {
            int slot = targetSlotBySelector[selectorOrdinal];
            int observed = Interlocked.CompareExchange(ref cells[slot], available, held);
            if (observed != held) throw new Exception("release did not observe held target");
            Interlocked.Increment(ref firstReleases);
        });
        if (firstReleases != 256) throw new Exception("first release count mismatch");

        int reverseReallocations = 0;
        Parallel.For(0, 256, new ParallelOptions { MaxDegreeOfParallelism = workers }, externalOrdinal =>
        {
            int selectorOrdinal = 255 - externalOrdinal;
            int selectorOffset = selectorOrdinal * KeyWidth;
            var recomputed = new byte[KeyWidth];
            DeriveTarget(table, states, locations, selectorOffset, mask, recomputed);
            if (!KeyEquals(recomputed, 0, targetBySelector, selectorOffset))
                throw new Exception("reallocation target changed under reversed external request order");
            int slot = Resolve(locations, recomputed, 0);
            if (slot != targetSlotBySelector[selectorOrdinal])
                throw new Exception("reallocation carrier slot changed for structural target");
            int observed = Interlocked.CompareExchange(ref cells[slot], held, available);
            if (observed != available) throw new Exception("reverse reallocation failed");
            Interlocked.Increment(ref reverseReallocations);
        });
        if (reverseReallocations != 256) throw new Exception("reverse reallocation count mismatch");

        int finalReleases = 0;
        Parallel.For(0, 256, new ParallelOptions { MaxDegreeOfParallelism = workers }, selectorOrdinal =>
        {
            int slot = targetSlotBySelector[selectorOrdinal];
            int observed = Interlocked.CompareExchange(ref cells[slot], available, held);
            if (observed != held) throw new Exception("final release did not observe held target");
            Interlocked.Increment(ref finalReleases);
        });
        if (finalReleases != 256) throw new Exception("final release count mismatch");

        var allocationMap = new byte[256 * MapRecordWidth];
        for (int selectorOrdinal = 0; selectorOrdinal < 256; selectorOrdinal++)
        {
            int selectorOffset = selectorOrdinal * KeyWidth;
            int recordOffset = selectorOrdinal * MapRecordWidth;
            Array.Copy(locations, selectorOffset, allocationMap, recordOffset, KeyWidth);
            Array.Copy(targetBySelector, selectorOffset, allocationMap, recordOffset + KeyWidth, KeyWidth);
        }

        var finalState = new byte[256 * StateRecordWidth];
        int finalAvailable = 0;
        for (int location = 0; location < 256; location++)
        {
            int state = Volatile.Read(ref cells[location]);
            if ((byte)state != available) throw new Exception("allocator did not return full pool to available");
            finalAvailable++;
            Array.Copy(locations, location * KeyWidth, finalState, location * StateRecordWidth, KeyWidth);
            finalState[(location * StateRecordWidth) + KeyWidth] = (byte)state;
        }

        Directory.CreateDirectory(Path.GetDirectoryName(allocationMapPath) ?? ".");
        File.WriteAllBytes(allocationMapPath, allocationMap);
        File.WriteAllBytes(finalStatePath, finalState);

        var witness = new List<string>();
        witness.Add("NATIVE-ALLOCATOR-V1");
        witness.Add("locations=256");
        witness.Add("workers=" + workers);
        witness.Add("selector-width-relations=8");
        witness.Add("target-width-relations=8");
        witness.Add("target-derivation=elementwise-selector-domain-native-composition");
        witness.Add("target-derivation-bijection=PASS");
        witness.Add("first-allocation-transitions=" + firstTransitions);
        witness.Add("exhausted-pool-rejections=" + exhaustedRejections);
        witness.Add("first-release-transitions=" + firstReleases);
        witness.Add("reverse-order-reallocations=" + reverseReallocations);
        witness.Add("final-release-transitions=" + finalReleases);
        witness.Add("final-available-locations=" + finalAvailable);
        witness.Add("external-request-order-semantics=none");
        witness.Add("allocation-size-semantics=one-native-resource");
        witness.Add("pointer-semantics=none");
        witness.Add("integer-address-semantics=none");
        File.WriteAllLines(witnessPath, witness.ToArray());

        return "NATIVE_ALLOCATOR_V1=PASS"
            + ";LOCATIONS=256"
            + ";WORKERS=" + workers
            + ";ALLOCATIONS=" + firstTransitions
            + ";EXHAUSTED_REJECTIONS=" + exhaustedRejections
            + ";RELEASES=" + firstReleases
            + ";REALLOCATIONS=" + reverseReallocations
            + ";FINAL_RELEASES=" + finalReleases
            + ";FINAL_AVAILABLE=" + finalAvailable
            + ";BIJECTION=PASS";
    }
}
