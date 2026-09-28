using System;
using System.Collections.Generic;
using System.IO;

public static class NativeAttentionV1
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

    private static byte[] ReadPercepts(string path, byte[] states)
    {
        string[] lines = File.ReadAllLines(path);
        if (lines.Length < 6 || lines[0] != "NATIVE-PERCEPTION-V1")
            throw new Exception("native perception artifact missing");

        int samples = int.Parse(lines[1].Split('=')[1]);
        if (samples <= 0 || lines.Length != 5 + samples)
            throw new Exception("native perception sample count mismatch");

        var result = new byte[samples];
        for (int i = 0; i < samples; i++)
        {
            string[] parts = lines[4 + i].Split(';');
            int index = int.Parse(parts[0].Split('=')[1]);
            byte relation = byte.Parse(parts[1].Split('=')[1]);
            if (index != i) throw new Exception("native perception ordering mismatch");
            if (Array.IndexOf(states, relation) < 0)
                throw new Exception("perception relation outside native carrier");
            result[i] = relation;
        }
        return result;
    }

    public static string Focus(
        string tablePath,
        string perceptPath,
        string attentionPath,
        string witnessPath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");

        byte[] states = States(table);
        byte identity = Identity(table, states);
        byte[] percepts = ReadPercepts(perceptPath, states);

        var focused = new List<byte>();
        var witness = new List<string>();
        witness.Add("NATIVE-ATTENTION-V1");
        witness.Add("samples=" + percepts.Length);
        witness.Add("identity-carrier-label=" + identity);

        for (int i = 0; i < percepts.Length; i++)
        {
            bool isIdentity = percepts[i] == identity;
            if (!isIdentity) focused.Add(percepts[i]);

            witness.Add(
                "percept-index=" + i
                + ";structural-role=" + (isIdentity ? "identity" : "non-identity")
                + ";focus=" + (!isIdentity ? "selected" : "not-selected"));
        }

        if (focused.Count == 0)
            throw new Exception("fresh AMD percept stream contained no structural novelty");

        Directory.CreateDirectory(Path.GetDirectoryName(attentionPath) ?? ".");
        File.WriteAllBytes(attentionPath, focused.ToArray());
        File.WriteAllLines(witnessPath, witness.ToArray());

        byte folded = identity;
        for (int i = 0; i < focused.Count; i++)
            folded = Compose(table, states, folded, focused[i]);

        return "NATIVE_ATTENTION_V1=PASS"
            + ";SAMPLES=" + percepts.Length
            + ";FOCUSED=" + focused.Count
            + ";IGNORED=" + (percepts.Length - focused.Count)
            + ";FOCUSED_FOLD_ROLE=" + (folded == identity ? "identity" : "non-identity");
    }
}
