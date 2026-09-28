using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Threading.Tasks;

public static class NativeEventV1
{
    private const int KeyWidth = 8;
    private const int EventWidth = 9;
    private const int StoreRecordWidth = 9;

    private static byte[] States(byte[] table)
    {
        var unique = new List<byte>();
        for (int i = 0; i < table.Length; i++)
            if (!unique.Contains(table[i])) unique.Add(table[i]);
        if (unique.Count != 2) throw new Exception("expected exactly two native relation carriers");
        return unique.ToArray();
    }

    private static int IndexOf(byte[] states, byte value)
    {
        for (int i = 0; i < states.Length; i++)
            if (states[i] == value) return i;
        throw new Exception("value outside native relation carrier");
    }

    private static byte Compose(byte[] table, byte[] states, byte left, byte right)
    {
        return table[(IndexOf(states, left) * 2) + IndexOf(states, right)];
    }

    private static byte Identity(byte[] table, byte[] states)
    {
        var candidates = new List<byte>();
        foreach (byte candidate in states)
        {
            bool ok = true;
            foreach (byte x in states)
            {
                if (Compose(table, states, candidate, x) != x ||
                    Compose(table, states, x, candidate) != x)
                {
                    ok = false;
                    break;
                }
            }
            if (ok) candidates.Add(candidate);
        }
        if (candidates.Count != 1) throw new Exception("structural identity must be unique");
        return candidates[0];
    }

    private static bool KeyEquals(byte[] a, int offsetA, byte[] b, int offsetB)
    {
        for (int i = 0; i < KeyWidth; i++)
            if (a[offsetA + i] != b[offsetB + i]) return false;
        return true;
    }

    private static int ResolveTargetSlot(byte[] locations, byte[] events, int eventOffset)
    {
        int matches = 0;
        int slot = -1;
        for (int candidate = 0; candidate < 256; candidate++)
        {
            if (KeyEquals(locations, candidate * KeyWidth, events, eventOffset))
            {
                matches++;
                slot = candidate;
            }
        }
        if (matches != 1)
            throw new Exception("event target must resolve to exactly one native location");
        return slot;
    }

    private static byte[] BuildEvents(byte[] locations, byte identity, byte exchange, int eventsPerLocation)
    {
        var events = new byte[256 * eventsPerLocation * EventWidth];
        int eventIndex = 0;
        for (int location = 0; location < 256; location++)
        {
            int keyOffset = location * KeyWidth;
            for (int local = 0; local < eventsPerLocation; local++)
            {
                int eventOffset = eventIndex * EventWidth;
                Array.Copy(locations, keyOffset, events, eventOffset, KeyWidth);
                int selector = ((local * 11) + (location * 7) + (local / 3)) % 17;
                events[eventOffset + KeyWidth] = selector < 8 ? exchange : identity;
                eventIndex++;
            }
        }
        return events;
    }

    public static string Deliver(
        string tablePath,
        string locationsPath,
        string eventsPath,
        string finalStorePath,
        string witnessPath,
        int workers,
        int eventsPerLocation)
    {
        if (workers <= 0 || eventsPerLocation <= 0)
            throw new ArgumentOutOfRangeException();

        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("LAB21 relation table length mismatch");
        byte[] states = States(table);
        byte identity = Identity(table, states);
        byte exchange = states[0] == identity ? states[1] : states[0];

        byte[] locations = File.ReadAllBytes(locationsPath);
        if (locations.Length != 256 * KeyWidth)
            throw new Exception("native location artifact length mismatch");

        byte[] events = BuildEvents(locations, identity, exchange, eventsPerLocation);
        int eventCount = events.Length / EventWidth;
        var cells = new int[256];
        for (int i = 0; i < cells.Length; i++) cells[i] = identity;

        int cursor = -1;
        int completed = 0;
        long retries = 0;
        var delivered = new int[eventCount];

        Parallel.For(0, workers, worker =>
        {
            long localRetries = 0;
            while (true)
            {
                int eventIndex = Interlocked.Increment(ref cursor);
                if (eventIndex >= eventCount) break;

                int eventOffset = eventIndex * EventWidth;
                int targetSlot = ResolveTargetSlot(locations, events, eventOffset);
                byte action = events[eventOffset + KeyWidth];
                if (Array.IndexOf(states, action) < 0)
                    throw new Exception("event action escaped native relation carrier");

                while (true)
                {
                    int observed = Volatile.Read(ref cells[targetSlot]);
                    byte next = Compose(table, states, (byte)observed, action);
                    int actual = Interlocked.CompareExchange(ref cells[targetSlot], next, observed);
                    if (actual == observed) break;
                    localRetries++;
                }

                if (Interlocked.Exchange(ref delivered[eventIndex], 1) != 0)
                    throw new Exception("native event delivered more than once");
                Interlocked.Increment(ref completed);
            }
            Interlocked.Add(ref retries, localRetries);
        });

        if (completed != eventCount)
            throw new Exception("not every native event was delivered");
        for (int i = 0; i < delivered.Length; i++)
            if (delivered[i] != 1) throw new Exception("native event missing from delivery set");

        var expected = new byte[256];
        for (int i = 0; i < expected.Length; i++) expected[i] = identity;
        for (int eventIndex = 0; eventIndex < eventCount; eventIndex++)
        {
            int eventOffset = eventIndex * EventWidth;
            int targetSlot = ResolveTargetSlot(locations, events, eventOffset);
            byte action = events[eventOffset + KeyWidth];
            expected[targetSlot] = Compose(table, states, expected[targetSlot], action);
        }

        var finalStore = new byte[256 * StoreRecordWidth];
        for (int location = 0; location < 256; location++)
        {
            byte actual = (byte)Volatile.Read(ref cells[location]);
            if (actual != expected[location])
                throw new Exception("concurrent event delivery differs from canonical structural replay");
            int keyOffset = location * KeyWidth;
            int recordOffset = location * StoreRecordWidth;
            Array.Copy(locations, keyOffset, finalStore, recordOffset, KeyWidth);
            finalStore[recordOffset + KeyWidth] = actual;
        }

        Directory.CreateDirectory(Path.GetDirectoryName(eventsPath) ?? ".");
        File.WriteAllBytes(eventsPath, events);
        File.WriteAllBytes(finalStorePath, finalStore);

        var witness = new List<string>();
        witness.Add("NATIVE-EVENT-V1");
        witness.Add("workers=" + workers);
        witness.Add("locations=256");
        witness.Add("events-per-location=" + eventsPerLocation);
        witness.Add("submitted-events=" + eventCount);
        witness.Add("delivered-events=" + completed);
        witness.Add("delivery-retries=" + retries);
        witness.Add("event-target=width-8-native-relational-location-key");
        witness.Add("event-action=one-native-LAB21-relation-element");
        witness.Add("exactly-once-delivery=PASS");
        witness.Add("canonical-replay-equivalence=PASS");
        witness.Add("external-delivery-order-semantics=none");
        witness.Add("numeric-event-id-semantics=none");
        witness.Add("pointer-semantics=none");
        File.WriteAllLines(witnessPath, witness.ToArray());

        return "NATIVE_EVENT_V1=PASS"
            + ";WORKERS=" + workers
            + ";LOCATIONS=256"
            + ";EVENTS_PER_LOCATION=" + eventsPerLocation
            + ";EVENTS=" + eventCount
            + ";DELIVERED=" + completed
            + ";EXACTLY_ONCE=PASS"
            + ";REPLAY_EQUIVALENCE=PASS";
    }
}
