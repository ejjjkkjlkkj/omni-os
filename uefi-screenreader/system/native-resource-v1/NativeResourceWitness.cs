using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Threading.Tasks;

public static class NativeResourceV1
{
    private const int KeyWidth = 8;
    private const int RecordWidth = 9;

    private static byte[] States(byte[] table)
    {
        var unique = new List<byte>();
        for (int i = 0; i < table.Length; i++)
            if (!unique.Contains(table[i])) unique.Add(table[i]);
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
            {
                if (Compose(table, states, candidate, x) != x ||
                    Compose(table, states, x, candidate) != x)
                {
                    ok = false;
                    break;
                }
            }
            if (ok) candidates.Add(candidate);
        }
        if (candidates.Count != 1) throw new Exception("structural identity must be unique");
        return candidates[0];
    }

    private static bool KeyEquals(byte[] all, int offsetA, byte[] key, int offsetB)
    {
        for (int i = 0; i < KeyWidth; i++)
            if (all[offsetA + i] != key[offsetB + i]) return false;
        return true;
    }

    private static int ResolveCarrierSlot(byte[] locations, byte[] key, int keyOffset)
    {
        int matches = 0;
        int slot = -1;
        for (int candidate = 0; candidate < 256; candidate++)
        {
            if (KeyEquals(locations, candidate * KeyWidth, key, keyOffset))
            {
                matches++;
                slot = candidate;
            }
        }
        if (matches != 1)
            throw new Exception("resource target location must resolve exactly once");
        return slot;
    }

    public static string Exercise(
        string tablePath,
        string locationsPath,
        string finalResourcePath,
        string witnessPath,
        int contenders)
    {
        if (contenders < 2) throw new ArgumentOutOfRangeException("contenders");

        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("LAB21 relation table length mismatch");

        byte[] states = States(table);
        byte available = Identity(table, states);
        byte held = states[0] == available ? states[1] : states[0];

        if (Compose(table, states, available, held) != held)
            throw new Exception("claim action does not transform available role to held role");
        if (Compose(table, states, held, held) != available)
            throw new Exception("release action does not return held role to available role");

        byte[] locations = File.ReadAllBytes(locationsPath);
        if (locations.Length != 256 * KeyWidth)
            throw new Exception("native location artifact length mismatch");

        var cells = new int[256];
        for (int i = 0; i < cells.Length; i++) cells[i] = available;

        long claimTransitions = 0;
        long blockedObservations = 0;
        long releaseTransitions = 0;

        for (int target = 0; target < 256; target++)
        {
            int keyOffset = target * KeyWidth;
            int slot = ResolveCarrierSlot(locations, locations, keyOffset);
            int successfulClaims = 0;
            int blocked = 0;

            Parallel.For(0, contenders, contender =>
            {
                int observed = Interlocked.CompareExchange(ref cells[slot], held, available);
                if (observed == available) Interlocked.Increment(ref successfulClaims);
                else if (observed == held) Interlocked.Increment(ref blocked);
                else throw new Exception("resource cell escaped native relation carrier");
            });

            if (successfulClaims != 1)
                throw new Exception("resource claim must produce exactly one native transition");
            if (blocked != contenders - 1)
                throw new Exception("all competing claim observations must see held role");
            if ((byte)Volatile.Read(ref cells[slot]) != held)
                throw new Exception("claimed resource did not reach held role");

            Interlocked.Add(ref claimTransitions, successfulClaims);
            Interlocked.Add(ref blockedObservations, blocked);

            byte releaseTarget = Compose(table, states, held, held);
            int releaseObserved = Interlocked.CompareExchange(ref cells[slot], releaseTarget, held);
            if (releaseObserved != held)
                throw new Exception("resource release did not observe held role");
            if ((byte)Volatile.Read(ref cells[slot]) != available)
                throw new Exception("released resource did not return to available role");
            Interlocked.Increment(ref releaseTransitions);
        }

        var records = new byte[256 * RecordWidth];
        for (int location = 0; location < 256; location++)
        {
            int keyOffset = location * KeyWidth;
            int recordOffset = location * RecordWidth;
            Array.Copy(locations, keyOffset, records, recordOffset, KeyWidth);
            records[recordOffset + KeyWidth] = (byte)Volatile.Read(ref cells[location]);
            if (records[recordOffset + KeyWidth] != available)
                throw new Exception("final resource state must be structurally available");
        }

        Directory.CreateDirectory(Path.GetDirectoryName(finalResourcePath) ?? ".");
        File.WriteAllBytes(finalResourcePath, records);

        var witness = new List<string>();
        witness.Add("NATIVE-RESOURCE-V1");
        witness.Add("locations=256");
        witness.Add("contenders-per-location=" + contenders);
        witness.Add("claim-transitions=" + claimTransitions);
        witness.Add("blocked-claim-observations=" + blockedObservations);
        witness.Add("release-transitions=" + releaseTransitions);
        witness.Add("available-role=structural-identity");
        witness.Add("held-role=structural-non-identity");
        witness.Add("claim-action=structural-non-identity");
        witness.Add("release-action=structural-non-identity");
        witness.Add("target-selection-policy=not-established");
        witness.Add("pointer-semantics=none");
        witness.Add("integer-address-semantics=none");
        File.WriteAllLines(witnessPath, witness.ToArray());

        return "NATIVE_RESOURCE_V1=PASS"
            + ";LOCATIONS=256"
            + ";CONTENDERS=" + contenders
            + ";CLAIMS=" + claimTransitions
            + ";BLOCKED=" + blockedObservations
            + ";RELEASES=" + releaseTransitions
            + ";FINAL_AVAILABLE=256";
    }
}
