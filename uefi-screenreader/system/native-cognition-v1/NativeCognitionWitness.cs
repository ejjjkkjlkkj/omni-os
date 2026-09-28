using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;

public static class NativeCognitionV1
{
    private static byte[] States(byte[] table)
    {
        var s = table.Distinct().ToArray();
        if (s.Length != 2) throw new Exception("expected exactly two carrier labels");
        return s;
    }

    private static int IndexOf(byte[] states, byte value)
    {
        for (int i = 0; i < states.Length; i++) if (states[i] == value) return i;
        throw new Exception("state outside relation carrier");
    }

    private static byte Compose(byte[] table, byte[] states, byte left, byte right)
    {
        int li = IndexOf(states, left);
        int ri = IndexOf(states, right);
        return table[(li * 2) + ri];
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
        if (candidates.Count != 1) throw new Exception("identity must be structurally unique");
        return candidates[0];
    }

    private static byte Inverse(byte[] table, byte[] states, byte identity, byte state)
    {
        var candidates = new List<byte>();
        foreach (byte action in states)
        {
            if (Compose(table, states, state, action) == identity &&
                Compose(table, states, action, state) == identity)
                candidates.Add(action);
        }
        if (candidates.Count != 1) throw new Exception("inverse must be structurally unique");
        return candidates[0];
    }

    private static byte Fold(byte[] table, byte[] states, byte identity, byte[] journal)
    {
        byte state = identity;
        for (int i = 0; i < journal.Length; i++)
            state = Compose(table, states, state, journal[i]);
        return state;
    }

    private static Dictionary<string,string> Fields(string line)
    {
        var d = new Dictionary<string,string>(StringComparer.Ordinal);
        foreach (string part in line.Split(';'))
        {
            int p = part.IndexOf('=');
            if (p <= 0) throw new Exception("malformed manifest field");
            d.Add(part.Substring(0,p), part.Substring(p+1));
        }
        return d;
    }

    public static string Derive(string tablePath, string memoryRoot, string outputPath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("LAB21 relation table length mismatch");

        byte[] states = States(table);
        byte identity = Identity(table, states);

        string[] manifest = File.ReadAllLines(Path.Combine(memoryRoot, "manifest.txt"));
        if (manifest.Length < 6 || manifest[0] != "NATIVE-MEMORY-V1")
            throw new Exception("memory manifest missing");

        int streams = int.Parse(manifest[1].Split('=')[1]);
        if (streams <= 0 || manifest.Length != 5 + streams)
            throw new Exception("memory stream count mismatch");

        var plans = new List<string>();
        int validated = 0;

        for (int stream = 0; stream < streams; stream++)
        {
            var fields = Fields(manifest[5 + stream]);
            if (int.Parse(fields["stream"]) != stream) throw new Exception("stream ordering mismatch");

            byte[] journal = File.ReadAllBytes(Path.Combine(memoryRoot, "stream-" + stream.ToString("D2") + ".qmem"));
            byte observedFinal = Fold(table, states, identity, journal);
            byte manifestFinal = byte.Parse(fields["final"]);
            if (observedFinal != manifestFinal) throw new Exception("memory final mismatch");

            byte action = Inverse(table, states, identity, observedFinal);
            byte predicted = Compose(table, states, observedFinal, action);
            if (predicted != identity) throw new Exception("derived plan does not recover structural identity");

            plans.Add(
                "stream=" + stream +
                ";observed-final=" + observedFinal +
                ";derived-action=" + action +
                ";predicted-state=" + predicted +
                ";structural-goal=" + identity
            );
            validated++;
        }

        Directory.CreateDirectory(Path.GetDirectoryName(outputPath) ?? ".");
        var outLines = new List<string>();
        outLines.Add("NATIVE-COGNITION-V1");
        outLines.Add("streams=" + streams);
        outLines.Add("structural-goal-carrier-label=" + identity);
        outLines.AddRange(plans);
        File.WriteAllLines(outputPath, outLines.ToArray());

        return "NATIVE_COGNITION_V1=PASS"
            + ";STREAMS=" + streams
            + ";OBSERVATIONS=" + validated
            + ";PLANS=" + validated
            + ";PREDICTIONS=" + validated
            + ";STRUCTURAL_RECOVERY=PASS";
    }
}
