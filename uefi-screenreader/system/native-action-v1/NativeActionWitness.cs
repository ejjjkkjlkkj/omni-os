using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;

public static class NativeActionV1
{
    private static byte[] States(byte[] table)
    {
        var s=table.Distinct().ToArray();
        if (s.Length!=2) throw new Exception("expected exactly two carrier labels");
        return s;
    }

    private static int Idx(byte[] states, byte x)
    {
        for(int i=0;i<states.Length;i++) if(states[i]==x) return i;
        throw new Exception("state outside relation carrier");
    }

    private static byte Compose(byte[] table, byte[] states, byte a, byte b)
        => table[(Idx(states,a)*2)+Idx(states,b)];

    private static byte Identity(byte[] table, byte[] states)
    {
        var candidates=new List<byte>();
        foreach(byte e in states)
        {
            bool ok=true;
            foreach(byte x in states)
                if(Compose(table,states,e,x)!=x || Compose(table,states,x,e)!=x) { ok=false; break; }
            if(ok) candidates.Add(e);
        }
        if(candidates.Count!=1) throw new Exception("unique structural identity not found");
        return candidates[0];
    }

    private static byte Inverse(byte[] table, byte[] states, byte identity, byte observed)
    {
        var candidates=new List<byte>();
        foreach(byte a in states)
            if(Compose(table,states,observed,a)==identity && Compose(table,states,a,observed)==identity)
                candidates.Add(a);
        if(candidates.Count!=1) throw new Exception("unique structural inverse not found");
        return candidates[0];
    }

    private static byte FoldPrefix(byte[] table, byte[] states, byte identity, byte[] journal, int count)
    {
        if(count<0 || count>journal.Length) throw new ArgumentOutOfRangeException();
        byte state=identity;
        for(int i=0;i<count;i++) state=Compose(table,states,state,journal[i]);
        return state;
    }

    private static Dictionary<string,string> Fields(string line)
    {
        var d=new Dictionary<string,string>(StringComparer.Ordinal);
        foreach(string part in line.Split(';'))
        {
            int p=part.IndexOf('=');
            if(p<=0) throw new Exception("malformed manifest field");
            d.Add(part.Substring(0,p),part.Substring(p+1));
        }
        return d;
    }

    public static string Execute(string tablePath,string memoryRoot,string outputPath)
    {
        byte[] table=File.ReadAllBytes(tablePath);
        if(table.Length!=4) throw new Exception("relation table length mismatch");
        byte[] states=States(table);
        byte identity=Identity(table,states);

        string[] manifest=File.ReadAllLines(Path.Combine(memoryRoot,"manifest.txt"));
        if(manifest.Length<6 || manifest[0]!="NATIVE-MEMORY-V1") throw new Exception("memory manifest missing");

        int streams=int.Parse(manifest[1].Split('=')[1]);
        if(manifest.Length!=5+streams) throw new Exception("manifest stream mismatch");

        var outLines=new List<string>();
        outLines.Add("NATIVE-ACTION-V1");
        outLines.Add("streams="+streams);
        outLines.Add("structural-goal-carrier-label="+identity);

        int actions=0;
        for(int stream=0;stream<streams;stream++)
        {
            var fields=Fields(manifest[5+stream]);
            byte[] journal=File.ReadAllBytes(Path.Combine(memoryRoot,"stream-"+stream.ToString("D2")+".qmem"));

            var cuts=new List<int>();
            foreach(var kv in fields)
                if(kv.Key.Length>1 && kv.Key[0]=='c')
                    cuts.Add(int.Parse(kv.Key.Substring(1)));
            cuts.Sort();

            foreach(int cut in cuts)
            {
                byte observed=FoldPrefix(table,states,identity,journal,cut);
                byte expected=byte.Parse(fields["c"+cut]);
                if(observed!=expected) throw new Exception("prefix observation mismatch");

                byte action=Inverse(table,states,identity,observed);
                byte actual=Compose(table,states,observed,action);
                if(actual!=identity) throw new Exception("native action failed structural goal");

                outLines.Add(
                    "stream="+stream+
                    ";cut="+cut+
                    ";observed="+observed+
                    ";action="+action+
                    ";actual="+actual+
                    ";goal="+identity
                );
                actions++;
            }
        }

        Directory.CreateDirectory(Path.GetDirectoryName(outputPath) ?? ".");
        File.WriteAllLines(outputPath,outLines.ToArray());

        return "NATIVE_ACTION_V1=PASS"
            + ";STREAMS="+streams
            + ";ACTIONS="+actions
            + ";ACTUAL_GOAL_MATCHES="+actions;
    }
}
