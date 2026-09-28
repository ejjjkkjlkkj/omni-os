using System;
using System.Threading;
using System.Threading.Tasks;

public static class Lab14Witness
{
    public static string Run(int workers, int incrementsPerWorker, int casRounds)
    {
        int counter = 0;
        Parallel.For(0, workers, _ =>
        {
            for (int i = 0; i < incrementsPerWorker; i++)
                Interlocked.Increment(ref counter);
        });

        int expectedCounter = checked(workers * incrementsPerWorker);
        if (counter != expectedCounter)
            throw new Exception("atomic increment mismatch: " + counter + " != " + expectedCounter);

        for (int round = 0; round < casRounds; round++)
        {
            int cell = 0;
            int winners = 0;
            Parallel.For(0, workers, worker =>
            {
                int desired = worker + 1;
                int observed = Interlocked.CompareExchange(ref cell, desired, 0);
                if (observed == 0) Interlocked.Increment(ref winners);
            });

            if (winners != 1) throw new Exception("CAS winner mismatch at round " + round + ": " + winners);
            if (cell < 1 || cell > workers) throw new Exception("CAS final state out of range at round " + round);
        }

        return "LAB14_ATOMIC=PASS;WORKERS=" + workers
            + ";INCREMENTS_PER_WORKER=" + incrementsPerWorker
            + ";FINAL_COUNTER=" + counter
            + ";CAS_ROUNDS=" + casRounds;
    }
}
