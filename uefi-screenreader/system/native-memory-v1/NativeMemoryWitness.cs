using System;
using System.Collections.Generic;
using System.IO;
using System.Threading.Tasks;

public static class NativeMemoryV1
{
    private static byte[] States(byte[] table)
    {
        var set = new SortedSet<byte>(table);
        if (set.Count != 2) throw new Exception("expected exactly two external carrier labels");
        var states = new byte[2];
        set.CopyTo(states);
        return states;
    }

    private static int IndexOf(byte[] states, byte value)
    {
        if (value == states[0]) return 0;
        if (value == states[1]) return 1;
        throw new Exception("unknown external carrier label");
    }

    private static byte Compose(byte[] table, byte[] states, byte left, byte right)
    {
        return table[(IndexOf(states, left) * 2) + IndexOf(states, right)];
    }

    private static byte Identity(byte[] table, byte[] states)
    {
        int matches = 0;
        byte result = 0;
        for (int c = 0; c < 2; c++)
        {
            byte candidate = states[c];
            bool ok = true;
            for (int i = 0; i < 2; i++)
            {
                byte x = states[i];
                if (Compose(table, states, candidate, x) != x) ok = false;
                if (Compose(table, states, x, candidate) != x) ok = false;
            }
            if (ok) { result = candidate; matches++; }
        }
        if (matches != 1) throw new Exception("identity is not structurally unique");
        return result;
    }

    private static byte Fold(byte[] table, byte[] states, byte identity, byte[] journal, int count)
    {
        byte state = identity;
        for (int i = 0; i < count; i++)
            state = Compose(table, states, state, journal[i]);
        return state;
    }

    private static byte Swap(byte value, byte low, byte high)
    {
        if (value == low) return high;
        if (value == high) return low;
        throw new Exception("cannot swap unknown label");
    }

    private static byte[] SwapTable(byte[] table, byte[] states)
    {
        byte low = states[0];
        byte high = states[1];
        var transformed = new byte[4];
        for (int r = 0; r < 2; r++)
        for (int c = 0; c < 2; c++)
        {
            byte newLeft = states[r];
            byte newRight = states[c];
            byte oldLeft = Swap(newLeft, low, high);
            byte oldRight = Swap(newRight, low, high);
            byte oldOutput = Compose(table, states, oldLeft, oldRight);
            transformed[(r * 2) + c] = Swap(oldOutput, low, high);
        }
        return transformed;
    }

    private static int[] Cuts(int count)
    {
        return new int[] { 0, count / 4, count / 2, (count * 3) / 4, count - 1, count };
    }

    private static Dictionary<string,string> ParseLine(string line)
    {
        var d = new Dictionary<string,string>(StringComparer.Ordinal);
        foreach (string part in line.Split(';'))
        {
            int p = part.IndexOf('=');
            if (p <= 0) continue;
            d[part.Substring(0, p)] = part.Substring(p + 1);
        }
        return d;
    }

    private static void RequireDistinctHistories(string root, int streams)
    {
        var histories = new byte[streams][];
        for (int i = 0; i < streams; i++)
            histories[i] = File.ReadAllBytes(Path.Combine(root, "stream-" + i.ToString("D2") + ".qmem"));

        for (int i = 0; i < streams; i++)
        for (int j = i + 1; j < streams; j++)
        {
            if (histories[i].Length != histories[j].Length) continue;
            bool same = true;
            for (int k = 0; k < histories[i].Length; k++)
            {
                if (histories[i][k] != histories[j][k])
                {
                    same = false;
                    break;
                }
            }
            if (same) throw new Exception("memory histories are not distinct: " + i + " and " + j);
        }
    }

    public static string Write(string tablePath, string root, int streams, int eventsPerStream)
    {
        if (streams <= 0 || eventsPerStream < 4) throw new ArgumentOutOfRangeException();

        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("LAB21 relation table length mismatch");
        byte[] states = States(table);
        byte identity = Identity(table, states);
        byte exchange = states[0] == identity ? states[1] : states[0];

        Directory.CreateDirectory(root);
        string[] manifest = new string[streams];

        Parallel.For(0, streams, stream =>
        {
            var journal = new byte[eventsPerStream];
            byte state = identity;
            int exchangeEvents = 0;

            for (int i = 0; i < eventsPerStream; i++)
            {
                byte action = (((i * (stream + 3)) + (stream * 11) + (i / 17)) % 31 < 13) ? exchange : identity;
                journal[i] = action;
                if (action == exchange) exchangeEvents++;
                state = Compose(table, states, state, action);
            }

            string path = Path.Combine(root, "stream-" + stream.ToString("D2") + ".qmem");
            File.WriteAllBytes(path, journal);

            var parts = new List<string>();
            parts.Add("stream=" + stream);
            parts.Add("events=" + eventsPerStream);
            parts.Add("exchange-events=" + exchangeEvents);
            parts.Add("final=" + state);

            foreach (int cut in Cuts(eventsPerStream))
                parts.Add("c" + cut + "=" + Fold(table, states, identity, journal, cut));

            manifest[stream] = string.Join(";", parts);
        });

        var header = new List<string>();
        header.Add("NATIVE-MEMORY-V1");
        header.Add("streams=" + streams);
        header.Add("events-per-stream=" + eventsPerStream);
        header.Add("identity-carrier-label=" + identity);
        header.Add("exchange-carrier-label=" + exchange);
        header.AddRange(manifest);
        File.WriteAllLines(Path.Combine(root, "manifest.txt"), header.ToArray());
        RequireDistinctHistories(root, streams);

        long total = (long)streams * eventsPerStream;
        return "NATIVE_MEMORY_V1_WRITE=PASS"
            + ";STREAMS=" + streams
            + ";EVENTS_PER_STREAM=" + eventsPerStream
            + ";TOTAL_EVENTS=" + total
            + ";DISTINCT_HISTORIES=" + streams;
    }

    public static string ReadAndValidate(string tablePath, string root)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        byte[] states = States(table);
        byte identity = Identity(table, states);
        byte exchange = states[0] == identity ? states[1] : states[0];

        string[] lines = File.ReadAllLines(Path.Combine(root, "manifest.txt"));
        if (lines.Length < 5 || lines[0] != "NATIVE-MEMORY-V1")
            throw new Exception("manifest header mismatch");

        int streams = int.Parse(lines[1].Split('=')[1]);
        int eventsPerStream = int.Parse(lines[2].Split('=')[1]);
        if (lines.Length != streams + 5) throw new Exception("manifest stream count mismatch");

        byte[] swappedTable = SwapTable(table, states);
        byte[] swappedStates = States(swappedTable);
        byte swappedIdentity = Identity(swappedTable, swappedStates);
        byte swappedExchange = swappedStates[0] == swappedIdentity ? swappedStates[1] : swappedStates[0];

        long validatedEvents = 0;
        int recoveredPrefixes = 0;

        Parallel.For(0, streams, stream =>
        {
            var m = ParseLine(lines[5 + stream]);
            if (int.Parse(m["stream"]) != stream) throw new Exception("manifest stream ordering mismatch");

            byte[] journal = File.ReadAllBytes(Path.Combine(root, "stream-" + stream.ToString("D2") + ".qmem"));
            if (journal.Length != eventsPerStream) throw new Exception("journal length mismatch");

            byte finalState = Fold(table, states, identity, journal, journal.Length);
            if (finalState != byte.Parse(m["final"])) throw new Exception("replay final mismatch");

            foreach (int cut in Cuts(eventsPerStream))
            {
                byte recovered = Fold(table, states, identity, journal, cut);
                if (recovered != byte.Parse(m["c" + cut])) throw new Exception("prefix recovery mismatch");
            }

            byte reverse = finalState;
            for (int i = journal.Length - 1; i >= 0; i--)
                reverse = Compose(table, states, reverse, journal[i]);
            if (reverse != identity) throw new Exception("reverse recovery did not reach identity");

            var swappedJournal = new byte[journal.Length];
            for (int i = 0; i < journal.Length; i++)
                swappedJournal[i] = Swap(journal[i], states[0], states[1]);

            byte swappedFinal = Fold(swappedTable, swappedStates, swappedIdentity, swappedJournal, swappedJournal.Length);
            bool originalRoleIsIdentity = finalState == identity;
            bool swappedRoleIsIdentity = swappedFinal == swappedIdentity;
            bool originalRoleIsExchange = finalState == exchange;
            bool swappedRoleIsExchange = swappedFinal == swappedExchange;
            if (originalRoleIsIdentity != swappedRoleIsIdentity || originalRoleIsExchange != swappedRoleIsExchange)
                throw new Exception("global label exchange changed memory semantics");
        });

        RequireDistinctHistories(root, streams);
        validatedEvents = (long)streams * eventsPerStream;
        recoveredPrefixes = streams * Cuts(eventsPerStream).Length;

        return "NATIVE_MEMORY_V1_READ=PASS"
            + ";STREAMS=" + streams
            + ";EVENTS_PER_STREAM=" + eventsPerStream
            + ";VALIDATED_EVENTS=" + validatedEvents
            + ";RECOVERED_PREFIXES=" + recoveredPrefixes
            + ";REVERSE_RECOVERY=PASS"
            + ";GLOBAL_LABEL_EXCHANGE_INVARIANCE=PASS"
            + ";DISTINCT_HISTORIES=" + streams;
    }
}
