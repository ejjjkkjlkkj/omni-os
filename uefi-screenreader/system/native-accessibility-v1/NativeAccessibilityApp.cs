using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Windows.Forms;

public sealed class NativeAccessibilityForm : Form
{
    private readonly byte[] table;
    private readonly byte[] states;
    private readonly byte identity;
    private readonly byte exchange;
    private byte current;
    private readonly string tracePath;
    private readonly Label status;
    private readonly Button actionButton;
    private int activations;

    private static int IndexOf(byte[] states, byte value)
    {
        for (int i = 0; i < states.Length; i++) if (states[i] == value) return i;
        throw new Exception("state outside native relation carrier");
    }

    private byte Compose(byte left, byte right)
    {
        return table[(IndexOf(states, left) * 2) + IndexOf(states, right)];
    }

    private byte FindIdentity()
    {
        var candidates = new List<byte>();
        foreach (byte e in states)
        {
            bool ok = true;
            foreach (byte x in states)
            {
                if (ComposeWith(e, x) != x || ComposeWith(x, e) != x)
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

    private byte ComposeWith(byte left, byte right)
    {
        return table[(IndexOf(states, left) * 2) + IndexOf(states, right)];
    }

    private string RoleName(byte state)
    {
        if (state == identity) return "identity";
        if (state == exchange) return "exchange";
        throw new Exception("unknown native structural role");
    }

    private string HumanState()
    {
        return "Native state " + RoleName(current);
    }

    private void ApplyHumanState()
    {
        string text = HumanState();
        status.Text = text;
        status.AccessibleName = text;
        status.AccessibleDescription = "Current native structural state";
        status.AccessibleRole = AccessibleRole.StaticText;
    }

    private void Trace(string line)
    {
        File.AppendAllText(tracePath, line + Environment.NewLine);
    }

    private void ApplyNativeAction(object sender, EventArgs args)
    {
        if (current != identity)
            current = Compose(current, exchange);

        activations++;
        ApplyHumanState();
        Trace("activation=" + activations + ";role=" + RoleName(current));

        if (current != identity)
            throw new Exception("native action did not reach structural identity");
    }

    public NativeAccessibilityForm(string tablePath, string tracePath)
    {
        this.tracePath = tracePath;
        table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");
        var unique = new List<byte>();
        for (int i = 0; i < table.Length; i++)
            if (!unique.Contains(table[i])) unique.Add(table[i]);
        states = unique.ToArray();
        if (states.Length != 2) throw new Exception("expected exactly two native relation carriers");

        identity = FindIdentity();
        exchange = states[0] == identity ? states[1] : states[0];
        current = exchange;

        Text = "Native Accessibility V1";
        AccessibleName = "Native Accessibility V1";
        AccessibleDescription = "Accessible human-facing projection of the native structural state";
        StartPosition = FormStartPosition.CenterScreen;
        ClientSize = new Size(520, 180);
        KeyPreview = true;

        status = new Label();
        status.Name = "NativeStatus";
        status.AutoSize = false;
        status.Location = new Point(24, 24);
        status.Size = new Size(460, 40);
        status.Font = new Font(SystemFonts.MessageBoxFont.FontFamily, 14.0f);
        Controls.Add(status);

        actionButton = new Button();
        actionButton.Name = "NativeAction";
        actionButton.Text = "Apply native action";
        actionButton.AccessibleName = "Apply native action";
        actionButton.AccessibleDescription = "Apply the native structural action and move the state to identity";
        actionButton.AccessibleRole = AccessibleRole.PushButton;
        actionButton.Location = new Point(24, 86);
        actionButton.Size = new Size(250, 44);
        actionButton.TabIndex = 0;
        actionButton.TabStop = true;
        actionButton.Click += ApplyNativeAction;
        Controls.Add(actionButton);

        AcceptButton = actionButton;
        ApplyHumanState();

        File.WriteAllText(tracePath,
            "NATIVE-ACCESSIBILITY-V1" + Environment.NewLine +
            "initial-role=" + RoleName(current) + Environment.NewLine +
            "human-visible-initial=" + HumanState() + Environment.NewLine +
            "human-accessible-name-initial=" + status.AccessibleName + Environment.NewLine +
            "action-name=" + actionButton.AccessibleName + Environment.NewLine);

        Shown += delegate {
            actionButton.Focus();
            Trace("keyboard-focus-initial=" + (actionButton.Focused ? "PASS" : "FAIL"));
        };

        var safetyTimer = new Timer();
        safetyTimer.Interval = 60000;
        safetyTimer.Tick += delegate { Close(); };
        safetyTimer.Start();

        FormClosed += delegate {
            Trace("final-role=" + RoleName(current));
            Trace("activations=" + activations);
        };
    }
}

public static class NativeAccessibilityApp
{
    [STAThread]
    public static void Main(string[] args)
    {
        if (args.Length != 2) throw new ArgumentException("expected relation table path and trace path");
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.Run(new NativeAccessibilityForm(args[0], args[1]));
    }
}
