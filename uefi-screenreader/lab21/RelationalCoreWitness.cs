using System;
using System.Collections.Generic;
using System.IO;
using System.Security.Cryptography;
using System.Threading.Tasks;

public static class Lab21Witness
{
    private static int IndexOf(byte[] states, byte value)
    {
        if (value == states[0]) return 0;
        if (value == states[1]) return 1;
        throw new Exception("unknown external state label: " + value);
    }

    private static byte Apply(byte[] table, byte[] states, byte left, byte right)
    {
        int li = IndexOf(states, left);
        int ri = IndexOf(states, right);
        return table[(li * 2) + ri];
    }

    private static byte DiscoverIdentity(byte[] table, byte[] states)
    {
        int matches = 0;
        byte identity = 0;
        for (int c = 0; c < 2; c++)
        {
            byte candidate = states[c];
            bool ok = true;
            for (int i = 0; i < 2; i++)
            {
                byte x = states[i];
                if (Apply(table, states, candidate, x) != x) ok = false;
                if (Apply(table, states, x, candidate) != x) ok = false;
            }
            if (ok)
            {
                identity = candidate;
                matches++;
            }
        }
        if (matches != 1) throw new Exception("identity element is not structurally unique");
        return identity;
    }

    private static void Validate(byte[] table)
    {
        if (table == null || table.Length != 4) throw new Exception("table length must be 4");

        var distinct = new SortedSet<byte>();
        for (int i = 0; i < table.Length; i++) distinct.Add(table[i]);
        if (distinct.Count != 2) throw new Exception("table must expose exactly two external labels");

        byte[] states = new byte[2];
        distinct.CopyTo(states);

        for (int l = 0; l < 2; l++)
        for (int r = 0; r < 2; r++)
        {
            byte y = Apply(table, states, states[l], states[r]);
            if (y != states[0] && y != states[1]) throw new Exception("closure failure");
            if (Apply(table, states, states[l], states[r]) != Apply(table, states, states[r], states[l]))
                throw new Exception("commutativity failure");
        }

        byte identity = DiscoverIdentity(table, states);
        byte exchange = states[0] == identity ? states[1] : states[0];

        if (Apply(table, states, exchange, exchange) != identity)
            throw new Exception("exchange is not self-inverse");

        for (int a = 0; a < 2; a++)
        for (int b = 0; b < 2; b++)
        for (int c = 0; c < 2; c++)
        {
            byte left = Apply(table, states, Apply(table, states, states[a], states[b]), states[c]);
            byte right = Apply(table, states, states[a], Apply(table, states, states[b], states[c]));
            if (left != right) throw new Exception("associativity failure");
        }
    }

    private static byte Swap(byte value, byte low, byte high)
    {
        if (value == low) return high;
        if (value == high) return low;
        throw new Exception("cannot exchange unknown label");
    }

    private static byte[] GlobalLabelExchange(byte[] table)
    {
        var distinct = new SortedSet<byte>();
        for (int i = 0; i < table.Length; i++) distinct.Add(table[i]);
        if (distinct.Count != 2) throw new Exception("cannot exchange labels without two states");
        byte[] states = new byte[2];
        distinct.CopyTo(states);
        byte low = states[0];
        byte high = states[1];

        byte[] exchanged = new byte[4];
        for (int nr = 0; nr < 2; nr++)
        for (int nc = 0; nc < 2; nc++)
        {
            byte newLeft = states[nr];
            byte newRight = states[nc];
            byte oldLeft = Swap(newLeft, low, high);
            byte oldRight = Swap(newRight, low, high);
            byte oldOut = Apply(table, states, oldLeft, oldRight);
            exchanged[(nr * 2) + nc] = Swap(oldOut, low, high);
        }
        return exchanged;
    }

    private static string Hash(byte[] data)
    {
        using (var sha = SHA256.Create())
            return BitConverter.ToString(sha.ComputeHash(data)).Replace("-", "").ToLowerInvariant();
    }

    public static string Run(string tablePath, int workers, int repeatsPerWorker)
    {
        if (workers <= 0 || repeatsPerWorker <= 0) throw new ArgumentOutOfRangeException();

        byte[] table = File.ReadAllBytes(tablePath);
        if (Hash(table) != "10db5223d19bd1d58c2b8eb3c723b0ba104cf17564f9434e53e1b9e642fb3b37")
            throw new Exception("relation core artifact hash mismatch");

        Validate(table);
        byte[] exchanged = GlobalLabelExchange(table);
        Validate(exchanged);

        var originalStates = new SortedSet<byte>(table);
        byte[] os = new byte[2];
        originalStates.CopyTo(os);
        byte originalIdentity = DiscoverIdentity(table, os);

        var exchangedStates = new SortedSet<byte>(exchanged);
        byte[] es = new byte[2];
        exchangedStates.CopyTo(es);
        byte exchangedIdentity = DiscoverIdentity(exchanged, es);

        if (originalIdentity == exchangedIdentity)
            throw new Exception("global external label exchange did not exchange the identity label");

        Parallel.For(0, workers, worker =>
        {
            for (int i = 0; i < repeatsPerWorker; i++)
            {
                Validate(table);
                Validate(exchanged);
            }
        });

        return "LAB21_NATIVE_RELATION_CORE=PASS"
            + ";WORKERS=" + workers
            + ";REPEATS_PER_WORKER=" + repeatsPerWorker
            + ";ORIGINAL_IDENTITY_LABEL=" + originalIdentity
            + ";EXCHANGED_IDENTITY_LABEL=" + exchangedIdentity
            + ";TABLE_SHA256=" + Hash(table)
            + ";LABEL_EXCHANGE_INVARIANCE=PASS"
            + ";ASSOCIATIVITY=PASS"
            + ";IDENTITY_UNIQUE=PASS"
            + ";SELF_INVERSE=PASS";
    }
}
