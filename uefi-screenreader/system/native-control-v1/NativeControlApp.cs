using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Forms;

public sealed class NativeControlForm : Form
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
    private readonly byte current;
    private readonly byte goal;
    private readonly string resultPath;
    private readonly string tracePath;
    private readonly Label status;
    private readonly Button keepButton;
    private readonly Button applyButton;
    private int decisions;

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
        return value == identity ? "identity" : "exchange";
    }

    private byte DeriveAction(byte from, byte target)
    {
        var candidates = new List<byte>();
        foreach (byte action in states)
            if (Compose(from, action) == target) candidates.Add(action);
        if (candidates.Count != 1) throw new Exception("control action must be unique");
        return candidates[0];
    }

    protected override bool ProcessCmdKey(ref Message msg, Keys keyData)
    {
        if (keyData == Keys.Enter)
        {
            if (keepButton.Focused)
            {
                ExecuteDecision("keyboard-enter", current, "keep-current");
                return true;
            }
            if (applyButton.Focused)
            {
                ExecuteDecision("keyboard-enter", goal, "apply-goal");
                return true;
            }
        }
        return base.ProcessCmdKey(ref msg, keyData);
    }

    private void ExecuteDecision(string externalCarrier, byte target, string decisionKind)
    {
        if (decisions != 0) throw new Exception("each control witness accepts exactly one human decision");

        byte action = DeriveAction(current, target);
        byte actual = Compose(current, action);
        if (actual != target) throw new Exception("native control action missed selected target");

        decisions++;
        Directory.CreateDirectory(Path.GetDirectoryName(resultPath) ?? ".");
        File.WriteAllBytes(resultPath, new byte[] { actual });

        string message =
            "Native control " + decisionKind
            + ", result " + Role(actual);

        status.Text = message;
        status.AccessibleName = message;

        int running = nvdaController_testIfRunning();
        if (running != 0) throw new Exception("NVDA not available, error=" + running);
        int speech = nvdaController_speakText(message);
        if (speech != 0) throw new Exception("NVDA speech failed, error=" + speech);
        int braille = nvdaController_brailleMessage(message);
        if (braille != 0) throw new Exception("NVDA braille failed, error=" + braille);

        File.AppendAllText(tracePath,
            "external-carrier=" + externalCarrier
            + ";decision-kind=" + decisionKind
            + ";current-role=" + Role(current)
            + ";goal-role=" + Role(goal)
            + ";selected-target-role=" + Role(target)
            + ";derived-action-role=" + Role(action)
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

    public NativeControlForm(
        string tablePath,
        string currentPath,
        string goalPath,
        string resultPath,
        string tracePath)
    {
        this.resultPath = resultPath;
        this.tracePath = tracePath;

        table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");

        var unique = new List<byte>();
        for (int i = 0; i < table.Length; i++)
            if (!unique.Contains(table[i])) unique.Add(table[i]);
        states = unique.ToArray();
        if (states.Length != 2) throw new Exception("expected exactly two native relation carriers");

        identity = FindIdentity();

        byte[] currentBytes = File.ReadAllBytes(currentPath);
        byte[] goalBytes = File.ReadAllBytes(goalPath);
        if (currentBytes.Length != 1 || goalBytes.Length != 1)
            throw new Exception("current and goal must each contain one native relation element");
        current = currentBytes[0];
        goal = goalBytes[0];
        if (Array.IndexOf(states, current) < 0 || Array.IndexOf(states, goal) < 0)
            throw new Exception("current or goal outside native relation carrier");

        Text = "Native Control V1";
        AccessibleName = "Native Control V1";
        AccessibleDescription = "Accessible human executive gate before native goal execution";
        StartPosition = FormStartPosition.CenterScreen;
        ClientSize = new Size(700, 240);

        status = new Label();
        status.Name = "NativeControlStatus";
        status.AutoSize = false;
        status.Location = new Point(24, 24);
        status.Size = new Size(650, 54);
        status.Font = new Font(SystemFonts.MessageBoxFont.FontFamily, 13.0f);
        status.Text = "Current " + Role(current) + ", selected goal " + Role(goal);
        status.AccessibleName = status.Text;
        status.AccessibleDescription = "Review current native state and selected native goal before execution";
        status.AccessibleRole = AccessibleRole.StaticText;
        Controls.Add(status);

        keepButton = new Button();
        keepButton.Name = "NativeControlKeep";
        keepButton.Text = "Keep current state";
        keepButton.AccessibleName = "Keep current native state";
        keepButton.AccessibleDescription = "Human veto: retain the current native state instead of applying the selected goal";
        keepButton.AccessibleRole = AccessibleRole.PushButton;
        keepButton.Location = new Point(24, 105);
        keepButton.Size = new Size(290, 46);
        keepButton.TabIndex = 0;
        keepButton.TabStop = true;
        keepButton.Click += delegate { ExecuteDecision("assistive-invoke", current, "keep-current"); };
        Controls.Add(keepButton);

        applyButton = new Button();
        applyButton.Name = "NativeControlApply";
        applyButton.Text = "Apply selected goal";
        applyButton.AccessibleName = "Apply selected native goal";
        applyButton.AccessibleDescription = "Human authorization: execute the native action required to reach the selected goal";
        applyButton.AccessibleRole = AccessibleRole.PushButton;
        applyButton.Location = new Point(334, 105);
        applyButton.Size = new Size(310, 46);
        applyButton.TabIndex = 1;
        applyButton.TabStop = true;
        applyButton.Click += delegate { ExecuteDecision("assistive-invoke", goal, "apply-goal"); };
        Controls.Add(applyButton);

        File.WriteAllText(tracePath,
            "NATIVE-CONTROL-V1" + Environment.NewLine
            + "current-role=" + Role(current) + Environment.NewLine
            + "goal-role=" + Role(goal) + Environment.NewLine
            + "human-decision-required=true" + Environment.NewLine);

        Shown += delegate { keepButton.Focus(); };
        FormClosed += delegate
        {
            File.AppendAllText(tracePath, "decisions=" + decisions + Environment.NewLine);
        };

        var safetyTimer = new Timer();
        safetyTimer.Interval = 60000;
        safetyTimer.Tick += delegate { Close(); };
        safetyTimer.Start();
    }
}

public static class NativeControlApp
{
    [STAThread]
    public static void Main(string[] args)
    {
        if (args.Length != 5)
            throw new ArgumentException("expected relation table current state goal state result path trace path");
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.Run(new NativeControlForm(args[0], args[1], args[2], args[3], args[4]));
    }
}
