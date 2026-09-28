using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Forms;

public sealed class NativeCorrectionReviewForm : Form
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
    private readonly byte[] history;
    private readonly byte[] plan;
    private readonly string decisionPath;
    private readonly string tracePath;
    private readonly int episodeIndex;
    private readonly byte actual;
    private readonly byte expected;
    private readonly byte plannedAction;
    private readonly Label status;
    private readonly Button approveButton;
    private readonly Button rejectButton;
    private bool decided;

    private static int IndexOf(byte[] values, byte value)
    {
        for (int i = 0; i < values.Length; i++) if (values[i] == value) return i;
        throw new Exception("value outside native relation carrier");
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

    private string ProposalMessage()
    {
        return "Correction proposal for episode " + (episodeIndex + 1)
            + ": actual " + Role(actual)
            + ", expected " + Role(expected)
            + ", planned action " + Role(plannedAction)
            + ". Press A to approve or R to reject.";
    }

    private void ProjectProposal()
    {
        string message = ProposalMessage();
        status.Text = message;
        status.AccessibleName = message;

        int running = nvdaController_testIfRunning();
        if (running != 0) throw new Exception("NVDA not available, error=" + running);
        int speech = nvdaController_speakText(message);
        if (speech != 0) throw new Exception("NVDA speech failed, error=" + speech);
        int braille = nvdaController_brailleMessage(message);
        if (braille != 0) throw new Exception("NVDA braille failed, error=" + braille);

        File.AppendAllText(tracePath,
            "projection=proposal"
            + ";episode-index=" + episodeIndex
            + ";actual-role=" + Role(actual)
            + ";expected-role=" + Role(expected)
            + ";planned-action-role=" + Role(plannedAction)
            + ";message=" + message
            + ";visible-text=" + status.Text
            + ";accessible-name=" + status.AccessibleName
            + ";speech-message=" + message
            + ";braille-message=" + message
            + ";nvda-running=" + running
            + ";nvda-speech=" + speech
            + ";nvda-braille=" + braille
            + ";executed=0"
            + Environment.NewLine);
    }

    private void Decide(byte selectedAction, string decisionKind, string carrier)
    {
        if (decided) return;
        if (selectedAction != plannedAction && selectedAction != identity)
            throw new Exception("human review selected action outside structural approve/reject choices");
        decided = true;

        string message = "Correction review " + decisionKind
            + ": selected action " + Role(selectedAction)
            + ", execution not performed.";

        File.WriteAllBytes(decisionPath, new byte[] { selectedAction });
        status.Text = message;
        status.AccessibleName = message;

        int running = nvdaController_testIfRunning();
        if (running != 0) throw new Exception("NVDA not available, error=" + running);
        int speech = nvdaController_speakText(message);
        if (speech != 0) throw new Exception("NVDA speech failed, error=" + speech);
        int braille = nvdaController_brailleMessage(message);
        if (braille != 0) throw new Exception("NVDA braille failed, error=" + braille);

        File.AppendAllText(tracePath,
            "projection=decision"
            + ";carrier=" + carrier
            + ";decision-kind=" + decisionKind
            + ";episode-index=" + episodeIndex
            + ";actual-role=" + Role(actual)
            + ";expected-role=" + Role(expected)
            + ";planned-action-role=" + Role(plannedAction)
            + ";selected-action-role=" + Role(selectedAction)
            + ";message=" + message
            + ";visible-text=" + status.Text
            + ";accessible-name=" + status.AccessibleName
            + ";speech-message=" + message
            + ";braille-message=" + message
            + ";nvda-running=" + running
            + ";nvda-speech=" + speech
            + ";nvda-braille=" + braille
            + ";executed=0"
            + Environment.NewLine);

        BeginInvoke(new Action(Close));
    }

    protected override bool ProcessCmdKey(ref Message msg, Keys keyData)
    {
        if (keyData == Keys.A)
        {
            Decide(plannedAction, "approve-proposal", "keyboard-a");
            return true;
        }
        if (keyData == Keys.R)
        {
            Decide(identity, "reject-proposal", "keyboard-r");
            return true;
        }
        return base.ProcessCmdKey(ref msg, keyData);
    }

    public NativeCorrectionReviewForm(
        string tablePath,
        string historyPath,
        string planPath,
        string decisionPath,
        string tracePath)
    {
        this.decisionPath = decisionPath;
        this.tracePath = tracePath;

        table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");

        var unique = new List<byte>();
        for (int i = 0; i < table.Length; i++)
            if (!unique.Contains(table[i])) unique.Add(table[i]);
        states = unique.ToArray();
        if (states.Length != 2) throw new Exception("expected exactly two native relation carriers");
        identity = FindIdentity();

        history = File.ReadAllBytes(historyPath);
        plan = File.ReadAllBytes(planPath);
        if (history.Length == 0 || history.Length % 4 != 0)
            throw new Exception("native episodic history framing mismatch");
        if (plan.Length != history.Length / 4)
            throw new Exception("native correction plan length mismatch");

        int found = -1;
        for (int i = 0; i < plan.Length; i++)
        {
            if (Array.IndexOf(states, plan[i]) < 0)
                throw new Exception("plan action outside native relation domain");
            if (plan[i] != identity)
            {
                if (found >= 0) throw new Exception("review witness expects one correction proposal");
                found = i;
            }
        }
        if (found < 0) throw new Exception("no correction proposal available for human review");

        episodeIndex = found;
        actual = history[(episodeIndex * 4) + 3];
        expected = history[(episodeIndex * 4) + 2];
        plannedAction = plan[episodeIndex];

        if (Array.IndexOf(states, actual) < 0 || Array.IndexOf(states, expected) < 0)
            throw new Exception("review episode outside native relation domain");
        if (Compose(actual, plannedAction) != expected)
            throw new Exception("planned action does not structurally repair the reviewed episode");

        Text = "Native Correction Review V1";
        AccessibleName = "Native Correction Review V1";
        AccessibleDescription = "Accessible human review of a non-executing native correction proposal";
        StartPosition = FormStartPosition.CenterScreen;
        ClientSize = new Size(900, 290);

        status = new Label();
        status.AutoSize = false;
        status.Location = new Point(24, 24);
        status.Size = new Size(840, 110);
        status.Font = new Font(SystemFonts.MessageBoxFont.FontFamily, 12.0f);
        status.AccessibleRole = AccessibleRole.StaticText;
        status.AccessibleDescription = "Native correction proposal and human decision status";
        Controls.Add(status);

        approveButton = new Button();
        approveButton.Text = "Approve correction proposal";
        approveButton.AccessibleName = "Approve native correction proposal";
        approveButton.AccessibleDescription = "Approve the proposed native correction action without executing it";
        approveButton.AccessibleRole = AccessibleRole.PushButton;
        approveButton.Location = new Point(24, 165);
        approveButton.Size = new Size(380, 52);
        approveButton.TabIndex = 0;
        approveButton.Click += delegate { Decide(plannedAction, "approve-proposal", "assistive-approve"); };
        Controls.Add(approveButton);

        rejectButton = new Button();
        rejectButton.Text = "Reject correction proposal";
        rejectButton.AccessibleName = "Reject native correction proposal";
        rejectButton.AccessibleDescription = "Reject the proposed correction and select the native identity action without executing it";
        rejectButton.AccessibleRole = AccessibleRole.PushButton;
        rejectButton.Location = new Point(430, 165);
        rejectButton.Size = new Size(380, 52);
        rejectButton.TabIndex = 1;
        rejectButton.Click += delegate { Decide(identity, "reject-proposal", "assistive-reject"); };
        Controls.Add(rejectButton);

        File.WriteAllText(tracePath,
            "NATIVE-CORRECTION-REVIEW-V1" + Environment.NewLine
            + "execution=forbidden" + Environment.NewLine
            + "autonomous-correction-execution=false" + Environment.NewLine);

        Shown += delegate
        {
            approveButton.Focus();
            ProjectProposal();
        };

        var timer = new Timer();
        timer.Interval = 60000;
        timer.Tick += delegate { Close(); };
        timer.Start();
    }
}

public static class NativeCorrectionReviewApp
{
    [STAThread]
    public static void Main(string[] args)
    {
        if (args.Length != 5)
            throw new ArgumentException("expected relation table history plan decision trace");
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.Run(new NativeCorrectionReviewForm(args[0], args[1], args[2], args[3], args[4]));
    }
}
