using System;
using System.Collections.Generic;
using System.IO;

public static class NativeStoreV1
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

    private static bool KeyEquals(byte[] a, int offsetA, byte[] b, int offsetB)
    {
        for (int i = 0; i < KeyWidth; i++)
            if (a[offsetA + i] != b[offsetB + i]) return false;
        return true;
    }

    private static byte FoldKey(byte[] table, byte[] states, byte identity, byte[] keySource, int keyOffset)
    {
        byte state = identity;
        for (int i = 0; i < KeyWidth; i++)
            state = Compose(table, states, state, keySource[keyOffset + i]);
        return state;
    }

    private static byte Lookup(byte[] store, byte[] keySource, int keyOffset)
    {
        if (store.Length % RecordWidth != 0) throw new Exception("native store record framing mismatch");
        int records = store.Length / RecordWidth;
        int matches = 0;
        byte result = 0;

        for (int record = 0; record < records; record++)
        {
            int offset = record * RecordWidth;
            if (KeyEquals(store, offset, keySource, keyOffset))
            {
                result = store[offset + KeyWidth];
                matches++;
            }
        }

        if (matches != 1) throw new Exception("native location lookup must resolve exactly one cell");
        return result;
    }

    public static string ValidateMutated(
        string tablePath,
        string locationsPath,
        string mutatedStorePath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("LAB21 relation table length mismatch");
        byte[] states = States(table);
        byte identity = Identity(table, states);

        byte[] locations = File.ReadAllBytes(locationsPath);
        if (locations.Length != 256 * KeyWidth)
            throw new Exception("native location artifact length mismatch");
        byte[] store = File.ReadAllBytes(mutatedStorePath);
        if (store.Length != 256 * RecordWidth)
            throw new Exception("native store artifact length mismatch");

        int lookups = 0;
        for (int location = 0; location < 256; location++)
        {
            int keyOffset = location * KeyWidth;
            byte initial = FoldKey(table, states, identity, locations, keyOffset);
            byte action = locations[keyOffset];
            byte expected = Compose(table, states, initial, action);
            byte actual = Lookup(store, locations, keyOffset);
            if (actual != expected)
                throw new Exception("order-independent mutated store lookup mismatch");
            lookups++;
        }

        return "NATIVE_STORE_V1_VALIDATE_MUTATED=PASS;LOOKUPS=" + lookups;
    }

    public static string BuildAndMutate(
        string tablePath,
        string locationsPath,
        string initialStorePath,
        string mutatedStorePath,
        string witnessPath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("LAB21 relation table length mismatch");

        byte[] states = States(table);
        byte identity = Identity(table, states);

        byte[] locations = File.ReadAllBytes(locationsPath);
        if (locations.Length != 256 * KeyWidth)
            throw new Exception("native location artifact length mismatch");

        var initialStore = new byte[256 * RecordWidth];
        for (int location = 0; location < 256; location++)
        {
            int keyOffset = location * KeyWidth;
            int recordOffset = location * RecordWidth;

            for (int i = 0; i < KeyWidth; i++)
            {
                byte element = locations[keyOffset + i];
                if (Array.IndexOf(states, element) < 0)
                    throw new Exception("native location key escaped relation carrier");
                initialStore[recordOffset + i] = element;
            }

            initialStore[recordOffset + KeyWidth] =
                FoldKey(table, states, identity, locations, keyOffset);
        }

        int initialLookups = 0;
        for (int location = 0; location < 256; location++)
        {
            int keyOffset = location * KeyWidth;
            byte expected = FoldKey(table, states, identity, locations, keyOffset);
            byte actual = Lookup(initialStore, locations, keyOffset);
            if (actual != expected)
                throw new Exception("initial native store lookup mismatch");
            initialLookups++;
        }

        var mutatedStore = (byte[])initialStore.Clone();
        int mutations = 0;
        for (int location = 0; location < 256; location++)
        {
            int keyOffset = location * KeyWidth;
            int recordOffset = location * RecordWidth;
            byte before = mutatedStore[recordOffset + KeyWidth];
            byte action = locations[keyOffset];
            byte after = Compose(table, states, before, action);
            mutatedStore[recordOffset + KeyWidth] = after;
            mutations++;
        }

        int mutatedLookups = 0;
        for (int location = 0; location < 256; location++)
        {
            int keyOffset = location * KeyWidth;
            byte initial = FoldKey(table, states, identity, locations, keyOffset);
            byte action = locations[keyOffset];
            byte expected = Compose(table, states, initial, action);
            byte actual = Lookup(mutatedStore, locations, keyOffset);
            if (actual != expected)
                throw new Exception("mutated native store lookup mismatch");
            mutatedLookups++;
        }

        Directory.CreateDirectory(Path.GetDirectoryName(initialStorePath) ?? ".");
        File.WriteAllBytes(initialStorePath, initialStore);
        File.WriteAllBytes(mutatedStorePath, mutatedStore);

        var witness = new List<string>();
        witness.Add("NATIVE-STORE-V1");
        witness.Add("locations=256");
        witness.Add("key-width-relations=8");
        witness.Add("record-width-carrier-bytes=9");
        witness.Add("initial-lookups=" + initialLookups);
        witness.Add("mutations=" + mutations);
        witness.Add("mutated-lookups=" + mutatedLookups);
        witness.Add("lookup-semantics=elementwise-native-location-key-equality");
        witness.Add("state-semantics=LAB21-native-relation-element");
        witness.Add("mutation-semantics=LAB21-native-composition");
        witness.Add("pointer-semantics=none");
        witness.Add("integer-address-semantics=none");
        File.WriteAllLines(witnessPath, witness.ToArray());

        return "NATIVE_STORE_V1=PASS"
            + ";LOCATIONS=256"
            + ";INITIAL_LOOKUPS=" + initialLookups
            + ";MUTATIONS=" + mutations
            + ";MUTATED_LOOKUPS=" + mutatedLookups
            + ";UNIQUE_LOOKUP=PASS"
            + ";POINTER_SEMANTICS=NONE";
    }
}
