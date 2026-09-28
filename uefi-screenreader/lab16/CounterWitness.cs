using System;
using System.Diagnostics;
using System.Threading.Tasks;

public static class Lab16Witness
{
    public static string Run(int workers, int samplesPerWorker)
    {
        long frequency = Stopwatch.Frequency;
        if (frequency <= 0) throw new Exception("invalid Stopwatch frequency");

        Parallel.For(0, workers, worker =>
        {
            long first = Stopwatch.GetTimestamp();
            long previous = first;
            long last = first;

            for (int i = 0; i < samplesPerWorker; i++)
            {
                long current = Stopwatch.GetTimestamp();
                if (current < previous)
                    throw new Exception("counter moved backward on worker " + worker + " at sample " + i);
                previous = current;
                last = current;
            }

            if (last <= first)
                throw new Exception("counter did not advance on worker " + worker);
        });

        return "LAB16_COUNTER=PASS;WORKERS=" + workers
            + ";SAMPLES_PER_WORKER=" + samplesPerWorker
            + ";FREQUENCY=" + frequency;
    }
}
