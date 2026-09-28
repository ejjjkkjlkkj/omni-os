using System;
using System.Security.Cryptography;
using System.Threading.Tasks;

public static class Lab15Witness
{
    private static byte Expected(int worker, int offset, int pass)
    {
        return (byte)((offset * 131 + worker * 17 + pass * 29) & 255);
    }

    public static string Run(int workers, int bytesPerWorker, int passes)
    {
        byte[] carrier = new byte[checked(workers * bytesPerWorker)];

        for (int pass = 0; pass < passes; pass++)
        {
            int capturedPass = pass;
            Parallel.For(0, workers, worker =>
            {
                int start = worker * bytesPerWorker;
                int end = start + bytesPerWorker;
                for (int p = start; p < end; p++)
                    carrier[p] = Expected(worker, p - start, capturedPass);
            });

            Parallel.For(0, workers, worker =>
            {
                int start = worker * bytesPerWorker;
                int end = start + bytesPerWorker;
                for (int p = start; p < end; p++)
                {
                    byte expected = Expected(worker, p - start, capturedPass);
                    if (carrier[p] != expected)
                        throw new Exception("memory mismatch worker=" + worker + " offset=" + (p - start) + " pass=" + capturedPass);
                }
            });
        }

        string hash;
        using (var sha = SHA256.Create())
            hash = BitConverter.ToString(sha.ComputeHash(carrier)).Replace("-", "").ToLowerInvariant();

        const string expectedHash = "a0e3895d6f3b3a78435e0c002e6e4b2264ccaadc14c4ea22be15fd4f5dc37fae";
        if (workers == 16 && bytesPerWorker == 2097152 && passes == 4 && hash != expectedHash)
            throw new Exception("final carrier hash mismatch: " + hash);

        return "LAB15_MEMORY=PASS;WORKERS=" + workers
            + ";BYTES_PER_WORKER=" + bytesPerWorker
            + ";PASSES=" + passes
            + ";TOTAL_BYTES=" + carrier.Length
            + ";SHA256=" + hash;
    }
}
