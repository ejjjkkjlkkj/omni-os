using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Forms;

public sealed class NativeInteractionForm : Form
{
    [DllImport("nvdaControllerClient.dll")]
    private static extern int nvdaController_testIfRunning();

    [DllImport("nvdaControllerClient.dll", CharSet = CharSet.Unicode)]
    private static extern int nvdaController_speakText(string text);

    [DllImport("nvdaControllerClient.dll", CharSet = CharSet.Unicode)]
    private static extern int nvdaController_brailleMessage(string message);

    private readonly byte[] table;
    private readonly byte[] states;
    private readonly byte identity;
    private readonly byte exchange;
    private byte current;
    private readonly string tracePath;
    private readonly Label status;
    private readonly Button actionButton;
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

    private byte FindInverse(byte observed)
    {
        var candidates = new List<byte>();
        foreach (byte action in states)
        {
            if (Compose(observed, action) == identity && Compose(action, observed) == identity)
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

    private string HumanProjection(byte value)
    {
        return "Native state " + Role(value);
    }

    private void ApplyHumanProjection()
    {
        string message = HumanProjection(current);
        status.Text = message;
        status.AccessibleName = message;
    }

    protected override bool ProcessCmdKey(ref Message msg, Keys keyData)
    {
        if (keyData == Keys.Enter && actionButton != null && actionButton.Focused)
        {
            NormalizeActAndProject("keyboard-enter");
            return true;
        }
        return base.ProcessCmdKey(ref msg, keyData);
    }

    private void NormalizeActAndProject(string externalCarrier)
    {
        if (acceptedInputs != 0)
            throw new Exception("each interaction witness process accepts exactly one external input");

        byte observed = current;
        byte nativeAction = FindInverse(observed);
        byte actual = Compose(observed, nativeAction);
        if (actual != identity) throw new Exception("native action missed structural goal");

        current = actual;
        acceptedInputs++;
        ApplyHumanProjection();

        string message = HumanProjection(current);
        int running = nvdaController_testIfRunning();
        if (running != 0) throw new Exception("NVDA not available, error=" + running);

        int speech = nvdaController_speakText(message);
        if (speech != 0) throw new Exception("NVDA speech failed, error=" + speech);

        int braille = nvdaController_brailleMessage(message);
        if (braille != 0) throw new Exception("NVDA braille failed, error=" + braille);

        File.AppendAllText(tracePath,
            "external-carrier=" + externalCarrier
            + ";observed-role=" + Role(observed)
            + ";normalized-action-role=" + Role(nativeAction)
            + ";actual-role=" + Role(actual)
            + ";visible-text=" + status.Text
            + ";accessible-name=" + status.AccessibleName
            + ";speech-message=" + message
            + ";braille-message=" + message
            + ";nvda-running=" + running
            + ";nvda-speech=" + speech
            + ";nvda-braille=" + braille
            + Environment.NewLine);

        BeginInvoke((MethodInvoker)delegate { Close(); });
    }

    public NativeInteractionForm(string tablePath, string tracePath)
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

        Text = "Native Interaction V1";
        AccessibleName = "Native Interaction V1";
        AccessibleDescription = "Accessible native input to action to output interaction cycle";
        StartPosition = FormStartPosition.CenterScreen;
        ClientSize = new Size(580, 190);

        status = new Label();
        status.Name = "NativeInteractionStatus";
        status.AutoSize = false;
        status.Location = new Point(24, 24);
        status.Size = new Size(520, 44);
        status.Font = new Font(SystemFonts.MessageBoxFont.FontFamily, 14.0f);
        status.AccessibleDescription = "Current native structural state";
        status.AccessibleRole = AccessibleRole.StaticText;
        Controls.Add(status);

        actionButton = new Button();
        actionButton.Name = "NativeInteractionAction";
        actionButton.Text = "Activate native interaction";
        actionButton.AccessibleName = "Activate native interaction";
        actionButton.AccessibleDescription = "Normalize the current input carrier into the native action";
        actionButton.AccessibleRole = AccessibleRole.PushButton;
        actionButton.Location = new Point(24, 92);
        actionButton.Size = new Size(300, 44);
        actionButton.TabIndex = 0;
        actionButton.TabStop = true;
        actionButton.Click += delegate { NormalizeActAndProject("assistive-invoke"); };
        Controls.Add(actionButton);

        ApplyHumanProjection();
        File.WriteAllText(tracePath,
            "NATIVE-INTERACTION-V1" + Environment.NewLine
            + "initial-role=" + Role(current) + Environment.NewLine
            + "initial-visible=" + status.Text + Environment.NewLine
            + "initial-accessible=" + status.AccessibleName + Environment.NewLine);

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

public static class NativeInteractionApp
{
    [STAThread]
    public static void Main(string[] args)
    {
        if (args.Length != 2) throw new ArgumentException("expected relation table path and trace path");
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.Run(new NativeInteractionForm(args[0], args[1]));
    }
}
