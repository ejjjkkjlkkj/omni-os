using System;
using System.Collections.Generic;
using System.IO;

public static class NativeEpisodeV1
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

    private static byte ReadOne(string path, byte[] states)
    {
        byte[] bytes = File.ReadAllBytes(path);
        if (bytes.Length != 1) throw new Exception("native state carrier must contain one relation element");
        if (Array.IndexOf(states, bytes[0]) < 0) throw new Exception("state carrier outside native relation domain");
        return bytes[0];
    }

    private static byte DeriveAction(byte[] table, byte[] states, byte current, byte target)
    {
        var actions = new List<byte>();
        foreach (byte candidate in states)
            if (Compose(table, states, current, candidate) == target)
                actions.Add(candidate);
        if (actions.Count != 1) throw new Exception("episode action must be structurally unique");
        return actions[0];
    }

    public static string Append(
        string tablePath,
        string currentPath,
        string goalPath,
        string targetPath,
        string resultPath,
        string historyPath,
        string witnessPath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");
        byte[] states = States(table);
        byte identity = Identity(table, states);

        byte current = ReadOne(currentPath, states);
        byte goal = ReadOne(goalPath, states);
        byte target = ReadOne(targetPath, states);
        byte result = ReadOne(resultPath, states);
        byte action = DeriveAction(table, states, current, target);

        byte predicted = Compose(table, states, current, action);
        if (predicted != target || result != target)
            throw new Exception("episode result is inconsistent with native transition");

        Directory.CreateDirectory(Path.GetDirectoryName(historyPath) ?? ".");
        using (var stream = new FileStream(historyPath, FileMode.Append, FileAccess.Write, FileShare.Read))
        {
            stream.WriteByte(current);
            stream.WriteByte(goal);
            stream.WriteByte(target);
            stream.WriteByte(result);
            stream.Flush(true);
        }

        File.WriteAllLines(witnessPath, new[] {
            "NATIVE-EPISODE-V1",
            "current-role=" + Role(current, identity),
            "goal-role=" + Role(goal, identity),
            "target-role=" + Role(target, identity),
            "derived-action-role=" + Role(action, identity),
            "result-role=" + Role(result, identity),
            "episode-elements=4"
        });

        long episodeCount = new FileInfo(historyPath).Length / 4;
        return "NATIVE_EPISODE_APPEND=PASS"
            + ";EPISODES=" + episodeCount
            + ";CURRENT_ROLE=" + Role(current, identity)
            + ";GOAL_ROLE=" + Role(goal, identity)
            + ";TARGET_ROLE=" + Role(target, identity)
            + ";ACTION_ROLE=" + Role(action, identity)
            + ";RESULT_ROLE=" + Role(result, identity);
    }

    public static string Replay(
        string tablePath,
        string historyPath,
        string replayPath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");
        byte[] states = States(table);
        byte identity = Identity(table, states);
        byte[] history = File.ReadAllBytes(historyPath);

        if (history.Length == 0 || history.Length % 4 != 0)
            throw new Exception("episodic history framing mismatch");

        int episodes = history.Length / 4;
        int goalTargets = 0;
        int currentTargets = 0;
        var lines = new List<string>();
        lines.Add("NATIVE-EPISODE-REPLAY-V1");
        lines.Add("episodes=" + episodes);

        for (int i = 0; i < episodes; i++)
        {
            byte current = history[(i * 4) + 0];
            byte goal = history[(i * 4) + 1];
            byte target = history[(i * 4) + 2];
            byte result = history[(i * 4) + 3];

            foreach (byte x in new[] { current, goal, target, result })
                if (Array.IndexOf(states, x) < 0)
                    throw new Exception("episode contains relation outside native domain");

            byte action = DeriveAction(table, states, current, target);
            byte predicted = Compose(table, states, current, action);
            if (predicted != target || result != target)
                throw new Exception("episodic replay mismatch at episode " + i);

            if (target == goal) goalTargets++;
            if (target == current) currentTargets++;

            lines.Add(
                "episode=" + i
                + ";current-role=" + Role(current, identity)
                + ";goal-role=" + Role(goal, identity)
                + ";target-role=" + Role(target, identity)
                + ";derived-action-role=" + Role(action, identity)
                + ";result-role=" + Role(result, identity));
        }

        File.WriteAllLines(replayPath, lines.ToArray());

        return "NATIVE_EPISODE_REPLAY=PASS"
            + ";EPISODES=" + episodes
            + ";GOAL_TARGETS=" + goalTargets
            + ";CURRENT_TARGETS=" + currentTargets;
    }
}
