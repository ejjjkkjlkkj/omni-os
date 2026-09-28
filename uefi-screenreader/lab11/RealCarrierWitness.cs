using System;
using System.Security.Cryptography;
using System.Threading.Tasks;

public static class Lab11Witness
{
    static readonly int[] Widths = new[] { 2, 4, 8, 16, 32, 64, 128, 256 };

    static string ExpectedHash(int width)
    {
        switch (width)
        {
            case 2: return "06eb7d6a69ee19e5fbdf749018d3d2abfa04bcbd1365db312eb86dc7169389b8";
            case 4: return "10db5223d19bd1d58c2b8eb3c723b0ba104cf17564f9434e53e1b9e642fb3b37";
            case 8: return "22f97a10117efa48887eb57f502823b581afafaee624d78af2620451d1ad5c31";
            case 16: return "d1c9053487d0fe37767d7df8297c9ba5ca3811b09c1606b706f60e720291614c";
            case 32: return "6a7109f48afd4406884c435207d8b9c7709b9d153001b65afc8220f7888fe62c";
            case 64: return "ad5bc614754bf2bee307f6ade5d655a9feb4b5d76608220cc57680e46088f32f";
            case 128: return "67f753181f4c0f0c58a87530af04684ec3d6b690779a1f0bd913b3dfa8560afc";
            case 256: return "4fb01cc48ebb4496c1a4306e6251a920a6d59e98091777e2aff15de7838db8e6";
            default: throw new Exception("unexpected width");
        }
    }

    static byte[] Exchange(byte[] src)
    {
        var dst = new byte[src.Length];
        for (int i = 0; i < src.Length; i++)
        {
            if (src[i] == 0) dst[i] = 255;
            else if (src[i] == 255) dst[i] = 0;
            else throw new Exception("unexpected witness state");
        }
        return dst;
    }

    static byte[] Derive(byte[] src)
    {
        var dst = new byte[src.Length];
        byte prev = src[src.Length - 1];
        for (int i = 0; i < src.Length; i++)
        {
            byte cur = src[i];
            if (cur != 0 && cur != 255) throw new Exception("unexpected witness state");
            dst[i] = (byte)(cur == prev ? 0 : 255);
            prev = cur;
        }
        return dst;
    }

    static byte[] Recover(byte[] rel, byte anchor)
    {
        var dst = new byte[rel.Length];
        dst[0] = anchor;
        for (int i = 1; i < rel.Length; i++)
        {
            if (rel[i] == 0) dst[i] = dst[i - 1];
            else if (rel[i] == 255) dst[i] = (byte)(dst[i - 1] == 0 ? 255 : 0);
            else throw new Exception("unexpected relation state");
        }
        byte closure = (byte)(dst[0] == dst[dst.Length - 1] ? 0 : 255);
        if (closure != rel[0]) throw new Exception("closure mismatch");
        return dst;
    }

    static byte[] Flip(byte[] src, int first, int second)
    {
        var dst = (byte[])src.Clone();
        dst[first] = (byte)(dst[first] == 0 ? 255 : 0);
        if (second >= 0) dst[second] = (byte)(dst[second] == 0 ? 255 : 0);
        return dst;
    }

    static bool Equal(byte[] a, byte[] b)
    {
        if (a.Length != b.Length) return false;
        for (int i = 0; i < a.Length; i++) if (a[i] != b[i]) return false;
        return true;
    }

    static string Hash(byte[] data)
    {
        using (var sha = SHA256.Create())
        {
            return BitConverter.ToString(sha.ComputeHash(data)).Replace("-", "").ToLowerInvariant();
        }
    }

    static void ValidateOnce()
    {
        byte[] material = new byte[] { 0, 255 };

        for (int stage = 0; stage < Widths.Length; stage++)
        {
            int width = Widths[stage];
            if (material.Length != width) throw new Exception("width mismatch at " + width);
            if (Hash(material) != ExpectedHash(width)) throw new Exception("hash mismatch at " + width);

            int zeros = 0;
            int opposites = 0;
            for (int k = 0; k < width; k++)
            {
                if (material[k] == 0) zeros++;
                else if (material[k] == 255) opposites++;
                else throw new Exception("unexpected witness state");
            }
            if (zeros != width / 2 || opposites != width / 2) throw new Exception("balance mismatch at " + width);

            var opposite = Exchange(material);
            var relation = Derive(material);

            if (!Equal(Derive(opposite), relation)) throw new Exception("global label exchange mismatch at " + width);
            if (!Equal(Recover(relation, material[0]), material)) throw new Exception("primary recovery mismatch at " + width);
            if (!Equal(Recover(relation, opposite[0]), opposite)) throw new Exception("opposite recovery mismatch at " + width);

            for (int i = 0; i < width; i++)
            {
                var changed = Derive(Flip(material, i, -1));
                for (int k = 0; k < width; k++)
                {
                    bool expected = k == i || k == ((i + 1) % width);
                    bool actual = changed[k] != relation[k];
                    if (actual != expected) throw new Exception("single locality mismatch width=" + width + " position=" + i + " relation=" + k);
                }
            }

            for (int i = 0; i < width; i++)
            {
                for (int j = i + 1; j < width; j++)
                {
                    var left = Flip(material, i, j);
                    var right = Flip(material, j, i);
                    if (!Equal(left, right)) throw new Exception("pair order mismatch width=" + width + " pair=" + i + "," + j);

                    var changed = Derive(left);
                    var expected = new bool[width];
                    expected[i] = !expected[i];
                    expected[(i + 1) % width] = !expected[(i + 1) % width];
                    expected[j] = !expected[j];
                    expected[(j + 1) % width] = !expected[(j + 1) % width];

                    for (int k = 0; k < width; k++)
                    {
                        bool actual = changed[k] != relation[k];
                        if (actual != expected[k]) throw new Exception("pair locality mismatch width=" + width + " pair=" + i + "," + j + " relation=" + k);
                    }

                    if (!Equal(Derive(Exchange(left)), changed)) throw new Exception("pair label exchange mismatch width=" + width + " pair=" + i + "," + j);
                }
            }

            if (stage + 1 < Widths.Length)
            {
                var next = new byte[width * 2];
                Array.Copy(material, 0, next, 0, width);
                Array.Copy(opposite, 0, next, width, width);
                material = next;
            }
        }
    }

    public static string RunSerial(int repeats)
    {
        if (repeats < 1) throw new ArgumentOutOfRangeException("repeats");
        for (int i = 0; i < repeats; i++) ValidateOnce();
        return "LAB11_SERIAL=PASS;REPEATS=" + repeats;
    }

    public static string RunParallel(int workers, int repeatsPerWorker)
    {
        if (workers < 1) throw new ArgumentOutOfRangeException("workers");
        if (repeatsPerWorker < 1) throw new ArgumentOutOfRangeException("repeatsPerWorker");
        Parallel.For(0, workers, worker =>
        {
            for (int i = 0; i < repeatsPerWorker; i++) ValidateOnce();
        });
        return "LAB11_PARALLEL=PASS;WORKERS=" + workers + ";REPEATS_PER_WORKER=" + repeatsPerWorker;
    }
}
