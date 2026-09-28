using System;
using System.IO;
using System.Security.Cryptography;

public static class Lab17Carrier
{
    public static string Write(string path, int length)
    {
        byte[] data = new byte[length];
        for (int i = 0; i < data.Length; i++)
            data[i] = (byte)((i * 197 + 43) & 255);

        File.WriteAllBytes(path, data);

        using (var sha = SHA256.Create())
            return BitConverter.ToString(sha.ComputeHash(data)).Replace("-", "").ToLowerInvariant();
    }
}
