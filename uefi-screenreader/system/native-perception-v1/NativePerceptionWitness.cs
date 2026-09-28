using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;

public static class NativePerceptionV1
{
    private sealed class Sample
    {
        public int Target;
        public uint Apic8;
        public uint Apic32;
        public int Core;
        public int ThreadsPerCore;
    }

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

    private static Dictionary<string,string> Fields(string text)
    {
        var d = new Dictionary<string,string>(StringComparer.Ordinal);
        foreach (string part in text.Split(','))
        {
            int p = part.IndexOf('=');
            if (p <= 0) throw new Exception("malformed external sample");
            d.Add(part.Substring(0, p), part.Substring(p + 1));
        }
        return d;
    }

    private static Sample ParseSample(string item)
    {
        int colon = item.IndexOf(':');
        if (colon <= 0) throw new Exception("malformed topology sample");
        int target = int.Parse(item.Substring(0, colon));
        var f = Fields(item.Substring(colon + 1));
        return new Sample
        {
            Target = target,
            Apic8 = uint.Parse(f["apic8"]),
            Apic32 = uint.Parse(f["apic32"]),
            Core = int.Parse(f["core"]),
            ThreadsPerCore = int.Parse(f["tpc"])
        };
    }

    public static string Normalize(string topologyResult, string tablePath, string rawStimulusPath, string perceptionPath)
    {
        if (topologyResult == null || !topologyResult.StartsWith("LAB22_AFFINITY_TOPOLOGY=PASS;", StringComparison.Ordinal))
            throw new Exception("external topology witness did not pass");

        int mapPos = topologyResult.IndexOf(";MAP=", StringComparison.Ordinal);
        if (mapPos < 0) throw new Exception("topology map missing");

        string map = topologyResult.Substring(mapPos + 5);
        string[] items = map.Split('|');
        if (items.Length != 16) throw new Exception("expected 16 external topology samples");

        var samples = items.Select(ParseSample).ToArray();
        var apic8 = new HashSet<uint>();
        var apic32 = new HashSet<uint>();

        for (int i = 0; i < samples.Length; i++)
        {
            if (samples[i].Target != i) throw new Exception("external sample ordering mismatch");
            if (!apic8.Add(samples[i].Apic8)) throw new Exception("duplicate external apic8 sample");
            if (!apic32.Add(samples[i].Apic32)) throw new Exception("duplicate external apic32 sample");
            if (samples[i].ThreadsPerCore <= 0) throw new Exception("invalid external topology grouping");
        }

        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");
        byte[] states = States(table);
        byte identity = Identity(table, states);
        byte exchange = states[0] == identity ? states[1] : states[0];

        var percepts = new byte[samples.Length];
        int identityPercepts = 0;
        int exchangePercepts = 0;

        for (int i = 0; i < samples.Length; i++)
        {
            bool sameExternalGroup = i == 0 || samples[i].Core == samples[i - 1].Core;
            percepts[i] = sameExternalGroup ? identity : exchange;
            if (percepts[i] == identity) identityPercepts++; else exchangePercepts++;
        }

        byte final = identity;
        for (int i = 0; i < percepts.Length; i++)
            final = Compose(table, states, final, percepts[i]);

        Directory.CreateDirectory(Path.GetDirectoryName(rawStimulusPath) ?? ".");
        Directory.CreateDirectory(Path.GetDirectoryName(perceptionPath) ?? ".");
        File.WriteAllText(rawStimulusPath, topologyResult + Environment.NewLine);

        var lines = new List<string>();
        lines.Add("NATIVE-PERCEPTION-V1");
        lines.Add("samples=" + percepts.Length);
        lines.Add("identity-carrier-label=" + identity);
        lines.Add("exchange-carrier-label=" + exchange);
        for (int i = 0; i < percepts.Length; i++)
            lines.Add("sample=" + i + ";relation=" + percepts[i]);
        lines.Add("final-native-state=" + final);
        File.WriteAllLines(perceptionPath, lines.ToArray());

        return "NATIVE_PERCEPTION_V1=PASS"
            + ";SAMPLES=" + percepts.Length
            + ";IDENTITY_PERCEPTS=" + identityPercepts
            + ";EXCHANGE_PERCEPTS=" + exchangePercepts
            + ";FINAL_NATIVE_STATE=" + final;
    }
}
