using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Threading.Tasks;

public static class Lab24Witness
{
    private static byte[] States(byte[] table)
    {
        var set = new SortedSet<byte>(table);
        if (set.Count != 2) throw new Exception("expected exactly two external labels");
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

    public static string Run(string tablePath, int workers, int rounds, int operationsPerWorker)
    {
        if (workers <= 0 || rounds <= 0 || operationsPerWorker <= 0)
            throw new ArgumentOutOfRangeException();

        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("LAB21 relation table length mismatch");

        byte[] states = States(table);
        byte identity = Identity(table, states);
        byte exchange = states[0] == identity ? states[1] : states[0];

        long totalTransitions = 0;
        long totalRetries = 0;
        int identityFinals = 0;
        int exchangeFinals = 0;

        for (int round = 0; round < rounds; round++)
        {
            int shared = identity;
            var workerCompositions = new byte[workers];
            var workerRetries = new long[workers];

            Parallel.For(0, workers, worker =>
            {
                byte local = identity;
                long retries = 0;

                for (int i = 0; i < operationsPerWorker; i++)
                {
                    byte action = ((i + worker + round) % 3 == 0) ? exchange : identity;
                    local = Compose(table, states, local, action);

                    while (true)
                    {
                        int observed = Volatile.Read(ref shared);
                        byte next = Compose(table, states, (byte)observed, action);
                        int actual = Interlocked.CompareExchange(ref shared, next, observed);
                        if (actual == observed) break;
                        retries++;
                    }
                }

                workerCompositions[worker] = local;
                workerRetries[worker] = retries;
            });

            byte expected = identity;
            for (int worker = 0; worker < workers; worker++)
            {
                expected = Compose(table, states, expected, workerCompositions[worker]);
                totalRetries += workerRetries[worker];
            }

            byte finalState = (byte)Volatile.Read(ref shared);
            if (finalState != expected)
                throw new Exception("native transition mismatch at round " + round);

            if (finalState == identity) identityFinals++;
            else if (finalState == exchange) exchangeFinals++;
            else throw new Exception("state escaped LAB21 closure");

            totalTransitions += (long)workers * operationsPerWorker;
        }

        if (identityFinals == 0 || exchangeFinals == 0)
            throw new Exception("both structural states were not reached");

        return "LAB24_NATIVE_TRANSITION_CORE=PASS"
            + ";WORKERS=" + workers
            + ";ROUNDS=" + rounds
            + ";OPERATIONS_PER_WORKER=" + operationsPerWorker
            + ";TOTAL_TRANSITIONS=" + totalTransitions
            + ";CAS_RETRIES=" + totalRetries
            + ";IDENTITY_FINALS=" + identityFinals
            + ";EXCHANGE_FINALS=" + exchangeFinals
            + ";SCHEDULE_ORDER_INDEPENDENCE=PASS"
            + ";STATE_CLOSURE=PASS";
    }
}
