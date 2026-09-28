using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Threading.Tasks;

public static class NativeSchedulerV1
{
    private static byte[] States(byte[] table)
    {
        var set = new SortedSet<byte>(table);
        if (set.Count != 2) throw new Exception("expected exactly two native relation carriers");
        var states = new byte[2];
        set.CopyTo(states);
        return states;
    }

    private static int IndexOf(byte[] states, byte value)
    {
        if (value == states[0]) return 0;
        if (value == states[1]) return 1;
        throw new Exception("native action outside relation carrier");
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

    private static byte[] BuildWorkload(byte identity, byte exchange, int count)
    {
        var workload = new byte[count];
        for (int i = 0; i < count; i++)
        {
            int selector = ((i * 17) + (i / 7) + ((i / 97) * 5)) % 23;
            workload[i] = selector < 10 ? exchange : identity;
        }
        return workload;
    }

    private static byte Fold(byte[] table, byte[] states, byte identity, byte[] actions)
    {
        byte state = identity;
        for (int i = 0; i < actions.Length; i++)
            state = Compose(table, states, state, actions[i]);
        return state;
    }

    public static string Run(
        string tablePath,
        string workloadPath,
        string dispatchPath,
        string witnessPath,
        int workers,
        int actionCount)
    {
        if (workers <= 0 || actionCount < workers * 16)
            throw new ArgumentOutOfRangeException();

        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("LAB21 relation table length mismatch");
        byte[] states = States(table);
        byte identity = Identity(table, states);
        byte exchange = states[0] == identity ? states[1] : states[0];

        byte[] workload = BuildWorkload(identity, exchange, actionCount);
        byte canonicalFinal = Fold(table, states, identity, workload);

        int identityActions = 0;
        int exchangeActions = 0;
        for (int i = 0; i < workload.Length; i++)
        {
            if (workload[i] == identity) identityActions++;
            else if (workload[i] == exchange) exchangeActions++;
            else throw new Exception("workload escaped native relation carrier");
        }
        if (identityActions == 0 || exchangeActions == 0)
            throw new Exception("scheduler workload must exercise both structural actions");

        int cursor = -1;
        int completed = -1;
        int shared = identity;
        long retries = 0;
        int[] dispatchOrder = new int[actionCount];
        int[] workerClaims = new int[workers];

        Parallel.For(0, workers, worker =>
        {
            long localRetries = 0;
            int localClaims = 0;

            while (true)
            {
                int index = Interlocked.Increment(ref cursor);
                if (index >= actionCount) break;

                byte action = workload[index];
                while (true)
                {
                    int observed = Volatile.Read(ref shared);
                    byte next = Compose(table, states, (byte)observed, action);
                    int actual = Interlocked.CompareExchange(ref shared, next, observed);
                    if (actual == observed) break;
                    localRetries++;
                }

                int slot = Interlocked.Increment(ref completed);
                if (slot < 0 || slot >= actionCount)
                    throw new Exception("scheduler completion slot escaped workload");
                dispatchOrder[slot] = index;
                localClaims++;
            }

            workerClaims[worker] = localClaims;
            Interlocked.Add(ref retries, localRetries);
        });

        if (completed + 1 != actionCount)
            throw new Exception("scheduler did not complete every submitted action");

        var seen = new bool[actionCount];
        for (int slot = 0; slot < dispatchOrder.Length; slot++)
        {
            int index = dispatchOrder[slot];
            if (index < 0 || index >= actionCount)
                throw new Exception("dispatch index outside workload");
            if (seen[index])
                throw new Exception("native action dispatched more than once");
            seen[index] = true;
        }
        for (int i = 0; i < seen.Length; i++)
            if (!seen[i]) throw new Exception("native action was not dispatched");

        byte scheduledFinal = (byte)Volatile.Read(ref shared);
        if (scheduledFinal != canonicalFinal)
            throw new Exception("scheduled final state differs from canonical native fold");

        int activeWorkers = 0;
        for (int i = 0; i < workerClaims.Length; i++)
            if (workerClaims[i] > 0) activeWorkers++;
        if (activeWorkers < 2)
            throw new Exception("scheduler witness did not distribute work across multiple workers");

        Directory.CreateDirectory(Path.GetDirectoryName(workloadPath) ?? ".");
        File.WriteAllBytes(workloadPath, workload);
        using (var writer = new BinaryWriter(File.Create(dispatchPath)))
        {
            for (int i = 0; i < dispatchOrder.Length; i++)
                writer.Write(dispatchOrder[i]);
        }

        var witness = new List<string>();
        witness.Add("NATIVE-SCHEDULER-V1");
        witness.Add("workers=" + workers);
        witness.Add("active-workers=" + activeWorkers);
        witness.Add("submitted-actions=" + actionCount);
        witness.Add("completed-actions=" + (completed + 1));
        witness.Add("identity-actions=" + identityActions);
        witness.Add("exchange-actions=" + exchangeActions);
        witness.Add("cas-retries=" + retries);
        witness.Add("canonical-final-role=" + (canonicalFinal == identity ? "identity" : "exchange"));
        witness.Add("scheduled-final-role=" + (scheduledFinal == identity ? "identity" : "exchange"));
        witness.Add("exactly-once-dispatch=PASS");
        witness.Add("schedule-order-semantic-invariance=PASS");
        File.WriteAllLines(witnessPath, witness.ToArray());

        return "NATIVE_SCHEDULER_V1=PASS"
            + ";WORKERS=" + workers
            + ";ACTIVE_WORKERS=" + activeWorkers
            + ";SUBMITTED=" + actionCount
            + ";COMPLETED=" + (completed + 1)
            + ";EXACTLY_ONCE=PASS"
            + ";ORDER_INVARIANCE=PASS"
            + ";CAS_RETRIES=" + retries;
    }
}
