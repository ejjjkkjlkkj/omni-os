using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Windows.Forms;

public sealed class NativeInputButton : Button
{
    public event EventHandler NativeKeyboardEnter;

    protected override void OnKeyDown(KeyEventArgs e)
    {
        if (e.KeyCode == Keys.Enter)
        {
            e.Handled = true;
            e.SuppressKeyPress = true;
            var handler = NativeKeyboardEnter;
            if (handler != null) handler(this, EventArgs.Empty);
            return;
        }
        base.OnKeyDown(e);
    }
}

public sealed class NativeInputForm : Form
{
    private readonly byte[] table;
    private readonly byte[] states;
    private readonly byte identity;
    private readonly byte exchange;
    private byte current;
    private readonly string tracePath;
    private readonly Label status;
    private readonly NativeInputButton actionButton;
    private int acceptedInputs;

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
                if (Compose(e, x) != x || Compose(x, e) != x)
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

    private byte FindInverse(byte state)
    {
        var candidates = new List<byte>();
        foreach (byte action in states)
        {
            if (Compose(state, action) == identity && Compose(action, state) == identity)
                candidates.Add(action);
        }
        if (candidates.Count != 1) throw new Exception("structural inverse must be unique");
        return candidates[0];
    }

    private string Role(byte value)
    {
        if (value == identity) return "identity";
        if (value == exchange) return "exchange";
        throw new Exception("unknown structural role");
    }

    private void ApplyState()
    {
        string text = "Native state " + Role(current);
        status.Text = text;
        status.AccessibleName = text;
    }

    protected override bool ProcessCmdKey(ref Message msg, Keys keyData)
    {
        if (keyData == Keys.Enter && actionButton != null && actionButton.Focused)
        {
            NormalizeAndApply("keyboard-enter");
            return true;
        }
        return base.ProcessCmdKey(ref msg, keyData);
    }

    private void NormalizeAndApply(string externalCarrier)
    {
        if (acceptedInputs != 0)
            throw new Exception("each witness process accepts exactly one external input");

        byte observed = current;
        byte nativeAction = FindInverse(observed);
        byte actual = Compose(observed, nativeAction);
        if (actual != identity) throw new Exception("normalized native action missed structural goal");

        current = actual;
        acceptedInputs++;
        ApplyState();

        File.AppendAllText(tracePath,
            "external-carrier=" + externalCarrier
            + ";observed-role=" + Role(observed)
            + ";normalized-action-role=" + Role(nativeAction)
            + ";actual-role=" + Role(actual)
            + Environment.NewLine);

        BeginInvoke((MethodInvoker)delegate { Close(); });
    }

    public NativeInputForm(string tablePath, string tracePath)
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

        Text = "Native Input V1";
        AccessibleName = "Native Input V1";
        AccessibleDescription = "Multiple external input carriers normalized into one native structural action";
        StartPosition = FormStartPosition.CenterScreen;
        ClientSize = new Size(520, 180);

        status = new Label();
        status.AutoSize = false;
        status.Location = new Point(24, 24);
        status.Size = new Size(460, 40);
        status.AccessibleDescription = "Current native structural state";
        Controls.Add(status);

        actionButton = new NativeInputButton();
        actionButton.Text = "Activate native action";
        actionButton.AccessibleName = "Activate native action";
        actionButton.AccessibleDescription = "Normalize this input carrier into the native structural action";
        actionButton.AccessibleRole = AccessibleRole.PushButton;
        actionButton.Location = new Point(24, 86);
        actionButton.Size = new Size(260, 44);
        actionButton.TabIndex = 0;
        actionButton.TabStop = true;
        actionButton.Click += delegate { NormalizeAndApply("assistive-invoke"); };
        actionButton.NativeKeyboardEnter += delegate { NormalizeAndApply("keyboard-enter"); };
        Controls.Add(actionButton);

        ApplyState();
        File.WriteAllText(tracePath,
            "NATIVE-INPUT-V1" + Environment.NewLine
            + "initial-role=" + Role(current) + Environment.NewLine
            + "visible-initial=" + status.Text + Environment.NewLine
            + "accessible-initial=" + status.AccessibleName + Environment.NewLine);

        Shown += delegate { actionButton.Focus(); };
        FormClosed += delegate
        {
            File.AppendAllText(tracePath,
                "final-role=" + Role(current) + Environment.NewLine
                + "accepted-inputs=" + acceptedInputs + Environment.NewLine);
        };

        var safetyTimer = new Timer();
        safetyTimer.Interval = 60000;
        safetyTimer.Tick += delegate { Close(); };
        safetyTimer.Start();
    }
}

public static class NativeInputApp
{
    [STAThread]
    public static void Main(string[] args)
    {
        if (args.Length != 2) throw new ArgumentException("expected relation table path and trace path");
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.Run(new NativeInputForm(args[0], args[1]));
    }
}
