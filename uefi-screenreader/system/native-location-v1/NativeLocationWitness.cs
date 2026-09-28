using System;
using System.Collections.Generic;
using System.IO;

public static class NativeLocationV1
{
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

    private static byte Swap(byte value, byte a, byte b)
    {
        if (value == a) return b;
        if (value == b) return a;
        throw new Exception("cannot exchange external label outside carrier");
    }

    private static byte[] SwapTable(byte[] table, byte[] states)
    {
        byte a = states[0];
        byte b = states[1];
        var transformed = new byte[4];

        for (int r = 0; r < 2; r++)
        for (int c = 0; c < 2; c++)
        {
            byte newLeft = states[r];
            byte newRight = states[c];
            byte oldLeft = Swap(newLeft, a, b);
            byte oldRight = Swap(newRight, a, b);
            byte oldOutput = Compose(table, states, oldLeft, oldRight);
            transformed[(r * 2) + c] = Swap(oldOutput, a, b);
        }

        return transformed;
    }

    private static bool EqualKey(byte[] all, int offsetA, int offsetB, int width)
    {
        for (int i = 0; i < width; i++)
            if (all[offsetA + i] != all[offsetB + i]) return false;
        return true;
    }

    private static string RolePattern(byte[] all, int offset, int width, byte identity)
    {
        var chars = new char[width];
        for (int i = 0; i < width; i++)
            chars[i] = all[offset + i] == identity ? 'I' : 'X';
        return new string(chars);
    }

    public static string Generate(
        string tablePath,
        string locationsPath,
        string witnessPath,
        int width)
    {
        if (width != 8) throw new ArgumentOutOfRangeException("width");

        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("LAB21 relation table length mismatch");

        byte[] states = States(table);
        byte identity = Identity(table, states);
        byte exchange = states[0] == identity ? states[1] : states[0];

        int locationCount = 1 << width;
        var all = new byte[locationCount * width];

        for (int externalOrdinal = 0; externalOrdinal < locationCount; externalOrdinal++)
        {
            int offset = externalOrdinal * width;
            for (int position = 0; position < width; position++)
            {
                bool chooseExchange = ((externalOrdinal >> position) & 1) != 0;
                all[offset + position] = chooseExchange ? exchange : identity;
            }
        }

        long pairChecks = 0;
        for (int a = 0; a < locationCount; a++)
        for (int b = a + 1; b < locationCount; b++)
        {
            pairChecks++;
            if (EqualKey(all, a * width, b * width, width))
                throw new Exception("native location keys are not unique");
        }

        byte[] swappedTable = SwapTable(table, states);
        byte[] swappedStates = States(swappedTable);
        byte swappedIdentity = Identity(swappedTable, swappedStates);

        for (int location = 0; location < locationCount; location++)
        {
            int offset = location * width;
            string originalPattern = RolePattern(all, offset, width, identity);

            var swappedKey = new byte[width];
            for (int position = 0; position < width; position++)
                swappedKey[position] = Swap(all[offset + position], states[0], states[1]);

            string swappedPattern = RolePattern(swappedKey, 0, width, swappedIdentity);
            if (!String.Equals(originalPattern, swappedPattern, StringComparison.Ordinal))
                throw new Exception("global label exchange changed native location role pattern");
        }

        Directory.CreateDirectory(Path.GetDirectoryName(locationsPath) ?? ".");
        File.WriteAllBytes(locationsPath, all);

        var witness = new List<string>();
        witness.Add("NATIVE-LOCATION-V1");
        witness.Add("width-relations=" + width);
        witness.Add("locations=" + locationCount);
        witness.Add("pairwise-uniqueness-checks=" + pairChecks);
        witness.Add("equality=elementwise-native-relation-equality");
        witness.Add("ordering-semantics=none");
        witness.Add("arithmetic-semantics=none");
        witness.Add("pointer-semantics=none");
        witness.Add("global-label-exchange-invariance=PASS");
        witness.Add("first-role-pattern=" + RolePattern(all, 0, width, identity));
        witness.Add("last-role-pattern=" + RolePattern(all, (locationCount - 1) * width, width, identity));
        File.WriteAllLines(witnessPath, witness.ToArray());

        return "NATIVE_LOCATION_V1=PASS"
            + ";WIDTH=" + width
            + ";LOCATIONS=" + locationCount
            + ";PAIR_CHECKS=" + pairChecks
            + ";UNIQUE=PASS"
            + ";LABEL_EXCHANGE=PASS"
            + ";POINTER_SEMANTICS=NONE";
    }
}
