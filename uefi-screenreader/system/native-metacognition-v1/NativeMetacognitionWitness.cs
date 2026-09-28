using System;
using System.Collections.Generic;
using System.IO;

public static class NativeMetacognitionV1
{
    private static int IndexOf(byte[] states, byte value)
    {
        for (int i = 0; i < states.Length; i++) if (states[i] == value) return i;
        throw new Exception("state outside native relation carrier");
    }

    private static byte Compose(byte[] table, byte[] states, byte left, byte right)
    {
        return table[(IndexOf(states, left) * 2) + IndexOf(states, right)];
    }

    private static byte[] States(byte[] table)
    {
        var unique = new List<byte>();
        for (int i = 0; i < table.Length; i++)
            if (!unique.Contains(table[i])) unique.Add(table[i]);
        if (unique.Count != 2) throw new Exception("expected exactly two native relation carriers");
        return unique.ToArray();
    }

    private static byte Identity(byte[] table, byte[] states)
    {
        var candidates = new List<byte>();
        foreach (byte e in states)
        {
            bool ok = true;
            foreach (byte x in states)
            {
                if (Compose(table, states, e, x) != x || Compose(table, states, x, e) != x)
                {
                    ok = false;
                    break;
                }
            }
            if (ok) candidates.Add(e);
        }
        if (candidates.Count != 1) throw new Exception("structural identity must be unique");
        return candidates[0];
    }

    private static byte RelationFromActualToExpected(
        byte[] table,
        byte[] states,
        byte actual,
        byte expected)
    {
        var candidates = new List<byte>();
        foreach (byte relation in states)
            if (Compose(table, states, actual, relation) == expected)
                candidates.Add(relation);
        if (candidates.Count != 1)
            throw new Exception("metacognitive discrepancy relation must be unique");
        return candidates[0];
    }

    private static string Role(byte value, byte identity)
    {
        return value == identity ? "identity" : "exchange";
    }

    public static string Inspect(
        string tablePath,
        string episodeHistoryPath,
        string discrepancyPath,
        string witnessPath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");

        byte[] states = States(table);
        byte identity = Identity(table, states);
        byte[] history = File.ReadAllBytes(episodeHistoryPath);
        if (history.Length == 0 || history.Length % 4 != 0)
            throw new Exception("episodic history framing mismatch");

        int episodes = history.Length / 4;
        var discrepancies = new byte[episodes];
        int identityCount = 0;
        int exchangeCount = 0;
        var lines = new List<string>();
        lines.Add("NATIVE-METACOGNITION-V1");
        lines.Add("episodes=" + episodes);

        for (int i = 0; i < episodes; i++)
        {
            byte target = history[(i * 4) + 2];
            byte actual = history[(i * 4) + 3];
            if (Array.IndexOf(states, target) < 0 || Array.IndexOf(states, actual) < 0)
                throw new Exception("episode target or actual outside native relation carrier");

            byte discrepancy = RelationFromActualToExpected(table, states, actual, target);
            discrepancies[i] = discrepancy;
            if (discrepancy == identity) identityCount++;
            else exchangeCount++;

            lines.Add(
                "episode=" + i
                + ";expected-role=" + Role(target, identity)
                + ";actual-role=" + Role(actual, identity)
                + ";discrepancy-role=" + Role(discrepancy, identity));
        }

        Directory.CreateDirectory(Path.GetDirectoryName(discrepancyPath) ?? ".");
        File.WriteAllBytes(discrepancyPath, discrepancies);
        File.WriteAllLines(witnessPath, lines.ToArray());

        return "NATIVE_METACOGNITION_V1=PASS"
            + ";EPISODES=" + episodes
            + ";IDENTITY_DISCREPANCIES=" + identityCount
            + ";EXCHANGE_DISCREPANCIES=" + exchangeCount;
    }
}
