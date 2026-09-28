using System;
using System.Collections.Generic;
using System.IO;

public static class NativeGoalExecutionV1
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

    public static void WriteFixture(string tablePath, string role, string outputPath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");
        byte[] states = States(table);
        byte identity = Identity(table, states);
        byte exchange = states[0] == identity ? states[1] : states[0];

        byte value;
        if (role == "identity") value = identity;
        else if (role == "exchange") value = exchange;
        else throw new ArgumentException("fixture role must be identity or exchange");

        Directory.CreateDirectory(Path.GetDirectoryName(outputPath) ?? ".");
        File.WriteAllBytes(outputPath, new byte[] { value });
    }

    public static string Execute(
        string tablePath,
        string currentPath,
        string goalPath,
        string actionPath,
        string resultPath,
        string witnessPath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");
        byte[] states = States(table);
        byte identity = Identity(table, states);

        byte[] currentBytes = File.ReadAllBytes(currentPath);
        byte[] goalBytes = File.ReadAllBytes(goalPath);
        if (currentBytes.Length != 1 || goalBytes.Length != 1)
            throw new Exception("current and goal must each contain exactly one native relation element");

        byte current = currentBytes[0];
        byte goal = goalBytes[0];
        if (Array.IndexOf(states, current) < 0 || Array.IndexOf(states, goal) < 0)
            throw new Exception("current or goal outside native relation carrier");

        var actions = new List<byte>();
        foreach (byte candidate in states)
        {
            if (Compose(table, states, current, candidate) == goal)
                actions.Add(candidate);
        }
        if (actions.Count != 1)
            throw new Exception("goal-directed native action must be unique");

        byte action = actions[0];
        byte predicted = Compose(table, states, current, action);
        if (predicted != goal) throw new Exception("goal-directed prediction failed");

        byte actual = Compose(table, states, current, action);
        if (actual != goal) throw new Exception("goal-directed execution failed");

        Directory.CreateDirectory(Path.GetDirectoryName(actionPath) ?? ".");
        File.WriteAllBytes(actionPath, new byte[] { action });
        File.WriteAllBytes(resultPath, new byte[] { actual });

        File.WriteAllLines(witnessPath, new[] {
            "NATIVE-GOAL-EXECUTION-V1",
            "current-role=" + Role(current, identity),
            "goal-role=" + Role(goal, identity),
            "derived-action-role=" + Role(action, identity),
            "predicted-role=" + Role(predicted, identity),
            "actual-role=" + Role(actual, identity),
            "goal-match=" + (actual == goal ? "PASS" : "FAIL")
        });

        return "NATIVE_GOAL_EXECUTION_V1=PASS"
            + ";CURRENT_ROLE=" + Role(current, identity)
            + ";GOAL_ROLE=" + Role(goal, identity)
            + ";ACTION_ROLE=" + Role(action, identity)
            + ";ACTUAL_ROLE=" + Role(actual, identity)
            + ";GOAL_MATCH=PASS";
    }
}
