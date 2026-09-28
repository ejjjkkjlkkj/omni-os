using System;
using System.Collections.Generic;
using System.IO;

public static class NativeCorrectionPlanV1
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
        return value == identity ? "identity" : "exchange";
    }

    public static string Plan(
        string tablePath,
        string historyPath,
        string discrepancyPath,
        string planPath,
        string witnessPath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");
        byte[] states = States(table);
        byte identity = Identity(table, states);

        byte[] history = File.ReadAllBytes(historyPath);
        byte[] discrepancies = File.ReadAllBytes(discrepancyPath);
        if (history.Length == 0 || history.Length % 4 != 0)
            throw new Exception("episodic history framing mismatch");
        int episodes = history.Length / 4;
        if (discrepancies.Length != episodes)
            throw new Exception("discrepancy stream length mismatch");

        var plan = new byte[episodes];
        var witness = new List<string>();
        witness.Add("NATIVE-CORRECTION-PLAN-V1");
        witness.Add("episodes=" + episodes);
        int corrections = 0;
        int noops = 0;

        for (int i = 0; i < episodes; i++)
        {
            byte expected = history[(i * 4) + 2];
            byte actual = history[(i * 4) + 3];
            byte discrepancy = discrepancies[i];
            if (Array.IndexOf(states, expected) < 0 ||
                Array.IndexOf(states, actual) < 0 ||
                Array.IndexOf(states, discrepancy) < 0)
                throw new Exception("correction input outside native relation domain");

            if (Compose(table, states, actual, discrepancy) != expected)
                throw new Exception("discrepancy does not structurally repair actual to expected");

            plan[i] = discrepancy;
            if (discrepancy == identity) noops++;
            else corrections++;

            witness.Add(
                "episode=" + i
                + ";actual-role=" + Role(actual, identity)
                + ";expected-role=" + Role(expected, identity)
                + ";planned-action-role=" + Role(discrepancy, identity));
        }

        Directory.CreateDirectory(Path.GetDirectoryName(planPath) ?? ".");
        File.WriteAllBytes(planPath, plan);
        File.WriteAllLines(witnessPath, witness.ToArray());

        return "NATIVE_CORRECTION_PLAN_V1=PASS"
            + ";EPISODES=" + episodes
            + ";CORRECTIONS=" + corrections
            + ";NOOPS=" + noops
            + ";EXECUTED=0";
    }
}
