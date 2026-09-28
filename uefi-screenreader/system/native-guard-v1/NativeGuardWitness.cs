using System;
using System.Collections.Generic;
using System.IO;

public static class NativeGuardV1
{
    private const int KeyWidth = 8;

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

    private static void Residual(
        byte[] table,
        byte[] states,
        byte[] target,
        int targetOffset,
        byte[] capability,
        int capabilityOffset,
        byte[] output,
        int outputOffset)
    {
        for (int i = 0; i < KeyWidth; i++)
            output[outputOffset + i] = Compose(
                table,
                states,
                target[targetOffset + i],
                capability[capabilityOffset + i]);
    }

    private static bool IsIdentityVector(byte[] vector, int offset, byte identity)
    {
        for (int i = 0; i < KeyWidth; i++)
            if (vector[offset + i] != identity) return false;
        return true;
    }

    public static string Build(
        string tablePath,
        string locationsPath,
        string matchingResidualsPath,
        string mismatchedResidualsPath,
        string witnessPath)
    {
        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("LAB21 relation table length mismatch");
        byte[] states = States(table);
        byte identity = Identity(table, states);
        byte exchange = states[0] == identity ? states[1] : states[0];

        byte[] locations = File.ReadAllBytes(locationsPath);
        if (locations.Length != 256 * KeyWidth)
            throw new Exception("native location artifact length mismatch");

        var matchingResiduals = new byte[locations.Length];
        var mismatchedResiduals = new byte[locations.Length];
        int matchingIdentityVectors = 0;
        int mismatchNonIdentityVectors = 0;

        for (int location = 0; location < 256; location++)
        {
            int offset = location * KeyWidth;

            Residual(
                table, states,
                locations, offset,
                locations, offset,
                matchingResiduals, offset);

            if (!IsIdentityVector(matchingResiduals, offset, identity))
                throw new Exception("matching structural capability did not reduce to identity vector");
            matchingIdentityVectors++;

            var mismatchCapability = new byte[KeyWidth];
            Array.Copy(locations, offset, mismatchCapability, 0, KeyWidth);
            mismatchCapability[0] = Compose(table, states, mismatchCapability[0], exchange);

            Residual(
                table, states,
                locations, offset,
                mismatchCapability, 0,
                mismatchedResiduals, offset);

            if (IsIdentityVector(mismatchedResiduals, offset, identity))
                throw new Exception("mismatched structural capability unexpectedly reduced to identity vector");
            if (mismatchedResiduals[offset] != exchange)
                throw new Exception("mismatch residual must expose structural non-identity at transformed position");
            for (int i = 1; i < KeyWidth; i++)
                if (mismatchedResiduals[offset + i] != identity)
                    throw new Exception("mismatch residual escaped single structural difference");
            mismatchNonIdentityVectors++;
        }

        Directory.CreateDirectory(Path.GetDirectoryName(matchingResidualsPath) ?? ".");
        File.WriteAllBytes(matchingResidualsPath, matchingResiduals);
        File.WriteAllBytes(mismatchedResidualsPath, mismatchedResiduals);

        var witness = new List<string>();
        witness.Add("NATIVE-GUARD-V1");
        witness.Add("locations=256");
        witness.Add("key-width-relations=8");
        witness.Add("matching-identity-residual-vectors=" + matchingIdentityVectors);
        witness.Add("mismatch-nonidentity-residual-vectors=" + mismatchNonIdentityVectors);
        witness.Add("guard-evaluation=elementwise-native-composition-residual");
        witness.Add("matching-capability-residual=all-structural-identity");
        witness.Add("mismatched-capability-residual=contains-structural-non-identity");
        witness.Add("boolean-authorization-semantics=none");
        witness.Add("protected-operation-execution=none");
        witness.Add("cryptographic-security-claim=none");
        witness.Add("pointer-semantics=none");
        witness.Add("integer-address-semantics=none");
        File.WriteAllLines(witnessPath, witness.ToArray());

        return "NATIVE_GUARD_V1=PASS"
            + ";LOCATIONS=256"
            + ";MATCHING_IDENTITY_RESIDUALS=" + matchingIdentityVectors
            + ";MISMATCH_NONIDENTITY_RESIDUALS=" + mismatchNonIdentityVectors
            + ";BOOLEAN_AUTH=NONE"
            + ";EXECUTED=0";
    }
}
