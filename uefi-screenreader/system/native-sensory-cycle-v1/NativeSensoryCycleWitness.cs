using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;

public static class NativeSensoryCycleV1
{
    private static byte[] States(byte[] table)
    {
        var states = table.Distinct().ToArray();
        if (states.Length != 2) throw new Exception("expected exactly two native relation carriers");
        return states;
    }

    private static int IndexOf(byte[] states, byte value)
    {
        for (int i = 0; i < states.Length; i++) if (states[i] == value) return i;
        throw new Exception("state outside native relation carrier");
    }

    private static byte Compose(byte[] table, byte[] states, byte left, byte right)
    {
        return table[(IndexOf(states, left) * 2) + IndexOf(states, right)];
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

    private static byte Inverse(byte[] table, byte[] states, byte identity, byte observed)
    {
        var candidates = new List<byte>();
        foreach (byte action in states)
        {
            if (Compose(table, states, observed, action) == identity &&
                Compose(table, states, action, observed) == identity)
                candidates.Add(action);
        }
        if (candidates.Count != 1) throw new Exception("structural inverse must be unique");
        return candidates[0];
    }

    private static byte Fold(byte[] table, byte[] states, byte start, byte[] actions, bool reverse)
    {
        byte state = start;
        if (reverse)
        {
            for (int i = actions.Length - 1; i >= 0; i--)
                state = Compose(table, states, state, actions[i]);
        }
        else
        {
            for (int i = 0; i < actions.Length; i++)
                state = Compose(table, states, state, actions[i]);
        }
        return state;
    }

    private static byte[] ReadPercepts(string path, byte identity, byte exchange)
    {
        string[] lines = File.ReadAllLines(path);
        if (lines.Length < 6 || lines[0] != "NATIVE-PERCEPTION-V1")
            throw new Exception("native perception artifact missing");

        int samples = int.Parse(lines[1].Split('=')[1]);
        if (samples <= 0 || lines.Length != 5 + samples)
            throw new Exception("native perception sample count mismatch");
        if (byte.Parse(lines[2].Split('=')[1]) != identity)
            throw new Exception("identity carrier mismatch");
        if (byte.Parse(lines[3].Split('=')[1]) != exchange)
            throw new Exception("exchange carrier mismatch");

        var percepts = new byte[samples];
        for (int i = 0; i < samples; i++)
        {
            string[] parts = lines[4 + i].Split(';');
            int sample = int.Parse(parts[0].Split('=')[1]);
            byte relation = byte.Parse(parts[1].Split('=')[1]);
            if (sample != i) throw new Exception("perception ordering mismatch");
            if (relation != identity && relation != exchange)
                throw new Exception("perception relation outside native carrier");
            percepts[i] = relation;
        }
        return percepts;
    }

    public static string Persist(string tablePath, string perceptionPath, string memoryPath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");
        byte[] states = States(table);
        byte identity = Identity(table, states);
        byte exchange = states[0] == identity ? states[1] : states[0];
        byte[] percepts = ReadPercepts(perceptionPath, identity, exchange);

        Directory.CreateDirectory(Path.GetDirectoryName(memoryPath) ?? ".");
        File.WriteAllBytes(memoryPath, percepts);

        byte observed = Fold(table, states, identity, percepts, false);
        return "NATIVE_SENSORY_MEMORY_WRITE=PASS"
            + ";PERCEPTS=" + percepts.Length
            + ";OBSERVED_FINAL=" + observed;
    }

    public static string CloseLoop(string tablePath, string memoryPath, string cyclePath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");
        byte[] states = States(table);
        byte identity = Identity(table, states);

        byte[] memory = File.ReadAllBytes(memoryPath);
        if (memory.Length != 16) throw new Exception("expected 16 remembered percepts");
        for (int i = 0; i < memory.Length; i++)
            if (Array.IndexOf(states, memory[i]) < 0)
                throw new Exception("remembered percept outside native carrier");

        byte observed = Fold(table, states, identity, memory, false);
        byte reverseRecovered = Fold(table, states, observed, memory, true);
        if (reverseRecovered != identity)
            throw new Exception("reverse recall did not recover structural identity");

        byte action = Inverse(table, states, identity, observed);
        byte predicted = Compose(table, states, observed, action);
        if (predicted != identity) throw new Exception("prediction missed structural goal");

        byte actual = Compose(table, states, observed, action);
        if (actual != identity) throw new Exception("native action missed structural goal");

        Directory.CreateDirectory(Path.GetDirectoryName(cyclePath) ?? ".");
        File.WriteAllLines(cyclePath, new[] {
            "NATIVE-SENSORY-CYCLE-V1",
            "remembered-percepts=" + memory.Length,
            "observed-final=" + observed,
            "reverse-recovered=" + reverseRecovered,
            "derived-action=" + action,
            "predicted-state=" + predicted,
            "actual-state=" + actual,
            "structural-goal=" + identity
        });

        return "NATIVE_SENSORY_CYCLE_V1=PASS"
            + ";PERCEPTS=" + memory.Length
            + ";RECALL=PASS"
            + ";REVERSE_RECOVERY=PASS"
            + ";PLAN=PASS"
            + ";PREDICTION=PASS"
            + ";ACTION=PASS"
            + ";GOAL=PASS";
    }
}
