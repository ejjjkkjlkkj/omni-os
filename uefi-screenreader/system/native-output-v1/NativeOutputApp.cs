using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Windows.Forms;

public sealed class NativeOutputForm : Form
{
    private readonly string tracePath;
    private readonly Label status;
    private readonly Button closeButton;
    private readonly string semanticMessage;

    private static int IndexOf(byte[] states, byte value)
    {
        for (int i = 0; i < states.Length; i++) if (states[i] == value) return i;
        throw new Exception("state outside native relation carrier");
    }

    private static byte Compose(byte[] table, byte[] states, byte left, byte right)
    {
        return table[(IndexOf(states, left) * 2) + IndexOf(states, right)];
    }

    private static byte FindIdentity(byte[] table, byte[] states)
    {
        var candidates = new List<byte>();
        foreach (byte e in states)
        {
            bool ok = true;
            foreach (byte x in states)
            {
                if (Compose(table, states, e, x) != x || Compose(table, states, x, e) != x)
                {
                    ok = false;
                    break;
                }
            }
            if (ok) candidates.Add(e);
        }
        if (candidates.Count != 1) throw new Exception("structural identity must be unique");
        return candidates[0];
    }

    private static string StructuralRole(byte value, byte identity, byte exchange)
    {
        if (value == identity) return "identity";
        if (value == exchange) return "exchange";
        throw new Exception("unknown structural role");
    }

    private static string HumanProjection(string role)
    {
        if (role == "identity") return "Native state identity";
        if (role == "exchange") return "Native state exchange";
        throw new Exception("unknown role projection");
    }

    public NativeOutputForm(string tablePath, string tracePath)
    {
        this.tracePath = tracePath;

        byte[] table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");

        var unique = new List<byte>();
        for (int i = 0; i < table.Length; i++)
            if (!unique.Contains(table[i])) unique.Add(table[i]);
        byte[] states = unique.ToArray();
        if (states.Length != 2) throw new Exception("expected exactly two native relation carriers");

        byte identity = FindIdentity(table, states);
        byte exchange = states[0] == identity ? states[1] : states[0];
        string role = StructuralRole(identity, identity, exchange);
        semanticMessage = HumanProjection(role);

        Text = "Native Output V1";
        AccessibleName = "Native Output V1";
        AccessibleDescription = "One native semantic state projected through equivalent human output carriers";
        StartPosition = FormStartPosition.CenterScreen;
        ClientSize = new Size(560, 190);

        status = new Label();
        status.Name = "NativeOutputStatus";
        status.AutoSize = false;
        status.Location = new Point(24, 24);
        status.Size = new Size(500, 44);
        status.Font = new Font(SystemFonts.MessageBoxFont.FontFamily, 14.0f);
        status.Text = semanticMessage;
        status.AccessibleName = semanticMessage;
        status.AccessibleDescription = "Current native state projected from one structural semantic source";
        status.AccessibleRole = AccessibleRole.StaticText;
        Controls.Add(status);

        closeButton = new Button();
        closeButton.Name = "NativeOutputClose";
        closeButton.Text = "Close";
        closeButton.AccessibleName = "Close native output";
        closeButton.AccessibleRole = AccessibleRole.PushButton;
        closeButton.Location = new Point(24, 92);
        closeButton.Size = new Size(180, 42);
        closeButton.TabIndex = 0;
        closeButton.Click += delegate { Close(); };
        Controls.Add(closeButton);

        File.WriteAllText(tracePath,
            "NATIVE-OUTPUT-V1" + Environment.NewLine
            + "native-role=" + role + Environment.NewLine
            + "semantic-message=" + semanticMessage + Environment.NewLine
            + "visible-text=" + status.Text + Environment.NewLine
            + "accessible-name=" + status.AccessibleName + Environment.NewLine);

        Shown += delegate
        {
            File.AppendAllText(tracePath,
                "window-shown=true" + Environment.NewLine
                + "status-handle-created=" + status.IsHandleCreated.ToString().ToLowerInvariant() + Environment.NewLine);
        };

        var timer = new Timer();
        timer.Interval = 60000;
        timer.Tick += delegate { Close(); };
        timer.Start();
    }
}

public static class NativeOutputApp
{
    [STAThread]
    public static void Main(string[] args)
    {
        if (args.Length != 2) throw new ArgumentException("expected relation table path and trace path");
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.Run(new NativeOutputForm(args[0], args[1]));
    }
}
