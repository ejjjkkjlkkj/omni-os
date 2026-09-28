using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Forms;

public sealed class NativeGoalForm : Form
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
    private readonly string goalPath;
    private readonly string tracePath;
    private readonly Label status;
    private readonly Button identityButton;
    private readonly Button exchangeButton;
    private int selections;

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

    private string Role(byte value)
    {
        if (value == identity) return "identity";
        if (value == exchange) return "exchange";
        throw new Exception("unknown structural role");
    }

    protected override bool ProcessCmdKey(ref Message msg, Keys keyData)
    {
        if (keyData == Keys.Enter)
        {
            if (identityButton.Focused)
            {
                SelectGoal("keyboard-enter", identity);
                return true;
            }
            if (exchangeButton.Focused)
            {
                SelectGoal("keyboard-enter", exchange);
                return true;
            }
        }
        return base.ProcessCmdKey(ref msg, keyData);
    }

    private void SelectGoal(string externalCarrier, byte goal)
    {
        if (selections != 0) throw new Exception("each goal witness process accepts exactly one selection");
        if (Array.IndexOf(states, goal) < 0) throw new Exception("goal outside native relation carrier");

        selections++;
        string role = Role(goal);
        string message = "Native goal " + role;

        status.Text = message;
        status.AccessibleName = message;

        Directory.CreateDirectory(Path.GetDirectoryName(goalPath) ?? ".");
        File.WriteAllBytes(goalPath, new byte[] { goal });

        int running = nvdaController_testIfRunning();
        if (running != 0) throw new Exception("NVDA not available, error=" + running);
        int speech = nvdaController_speakText(message);
        if (speech != 0) throw new Exception("NVDA speech failed, error=" + speech);
        int braille = nvdaController_brailleMessage(message);
        if (braille != 0) throw new Exception("NVDA braille failed, error=" + braille);

        File.AppendAllText(tracePath,
            "external-carrier=" + externalCarrier
            + ";selected-goal-role=" + role
            + ";persisted-role=" + role
            + ";goal-bytes=1"
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

    public NativeGoalForm(string tablePath, string goalPath, string tracePath)
    {
        this.goalPath = goalPath;
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

        Text = "Native Goal V1";
        AccessibleName = "Native Goal V1";
        AccessibleDescription = "Accessible human selection of a native structural goal";
        StartPosition = FormStartPosition.CenterScreen;
        ClientSize = new Size(640, 230);

        status = new Label();
        status.Name = "NativeGoalStatus";
        status.AutoSize = false;
        status.Location = new Point(24, 24);
        status.Size = new Size(580, 44);
        status.Font = new Font(SystemFonts.MessageBoxFont.FontFamily, 14.0f);
        status.Text = "Choose native goal";
        status.AccessibleName = "Choose native goal";
        status.AccessibleDescription = "Select one structural native goal";
        status.AccessibleRole = AccessibleRole.StaticText;
        Controls.Add(status);

        identityButton = new Button();
        identityButton.Name = "NativeGoalIdentity";
        identityButton.Text = "Set native goal identity";
        identityButton.AccessibleName = "Set native goal identity";
        identityButton.AccessibleDescription = "Persist the structural identity relation as the native goal";
        identityButton.AccessibleRole = AccessibleRole.PushButton;
        identityButton.Location = new Point(24, 94);
        identityButton.Size = new Size(270, 44);
        identityButton.TabIndex = 0;
        identityButton.TabStop = true;
        identityButton.Click += delegate { SelectGoal("assistive-invoke", identity); };
        Controls.Add(identityButton);

        exchangeButton = new Button();
        exchangeButton.Name = "NativeGoalExchange";
        exchangeButton.Text = "Set native goal exchange";
        exchangeButton.AccessibleName = "Set native goal exchange";
        exchangeButton.AccessibleDescription = "Persist the structural non-identity relation as the native goal";
        exchangeButton.AccessibleRole = AccessibleRole.PushButton;
        exchangeButton.Location = new Point(314, 94);
        exchangeButton.Size = new Size(290, 44);
        exchangeButton.TabIndex = 1;
        exchangeButton.TabStop = true;
        exchangeButton.Click += delegate { SelectGoal("assistive-invoke", exchange); };
        Controls.Add(exchangeButton);

        File.WriteAllText(tracePath,
            "NATIVE-GOAL-V1" + Environment.NewLine
            + "goal-domain-size=" + states.Length + Environment.NewLine
            + "identity-role=identity" + Environment.NewLine
            + "other-role=exchange" + Environment.NewLine);

        Shown += delegate { identityButton.Focus(); };
        FormClosed += delegate
        {
            File.AppendAllText(tracePath, "selections=" + selections + Environment.NewLine);
        };

        var safetyTimer = new Timer();
        safetyTimer.Interval = 60000;
        safetyTimer.Tick += delegate { Close(); };
        safetyTimer.Start();
    }
}

public static class NativeGoalApp
{
    [STAThread]
    public static void Main(string[] args)
    {
        if (args.Length != 3)
            throw new ArgumentException("expected relation table path goal path trace path");
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.Run(new NativeGoalForm(args[0], args[1], args[2]));
    }
}
