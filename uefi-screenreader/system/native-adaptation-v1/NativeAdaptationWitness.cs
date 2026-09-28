using System;
using System.Collections.Generic;
using System.IO;

public static class NativeAdaptationV1
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

    private static string Role(byte value, byte identity)
    {
        return value == identity ? "identity" : "non-identity";
    }

    public static string Update(
        string tablePath,
        string attentionPath,
        string statePath,
        string witnessPath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");

        byte[] states = States(table);
        byte identity = Identity(table, states);

        byte before = identity;
        if (File.Exists(statePath))
        {
            byte[] prior = File.ReadAllBytes(statePath);
            if (prior.Length != 1) throw new Exception("adaptive state length mismatch");
            if (Array.IndexOf(states, prior[0]) < 0)
                throw new Exception("adaptive state outside native relation carrier");
            before = prior[0];
        }

        byte[] focused = File.ReadAllBytes(attentionPath);
        if (focused.Length == 0) throw new Exception("attention stream must not be empty");

        byte after = before;
        for (int i = 0; i < focused.Length; i++)
        {
            if (Array.IndexOf(states, focused[i]) < 0)
                throw new Exception("focused relation outside native carrier");
            if (focused[i] == identity)
                throw new Exception("attention stream contains structural identity");
            after = Compose(table, states, after, focused[i]);
        }

        Directory.CreateDirectory(Path.GetDirectoryName(statePath) ?? ".");
        File.WriteAllBytes(statePath, new byte[] { after });

        File.WriteAllLines(witnessPath, new[] {
            "NATIVE-ADAPTATION-V1",
            "before-role=" + Role(before, identity),
            "focused-events=" + focused.Length,
            "after-role=" + Role(after, identity),
            "state-bytes=1"
        });

        return "NATIVE_ADAPTATION_V1=PASS"
            + ";BEFORE_ROLE=" + Role(before, identity)
            + ";FOCUSED=" + focused.Length
            + ";AFTER_ROLE=" + Role(after, identity);
    }
}
