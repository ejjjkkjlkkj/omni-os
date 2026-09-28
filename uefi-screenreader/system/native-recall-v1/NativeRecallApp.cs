using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Forms;

public sealed class NativeRecallButton : Button
{
    public event EventHandler RecallLeft;
    public event EventHandler RecallRight;

    protected override bool IsInputKey(Keys keyData)
    {
        Keys code = keyData & Keys.KeyCode;
        if (code == Keys.Left || code == Keys.Right) return true;
        return base.IsInputKey(keyData);
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        if (e.KeyCode == Keys.Left)
        {
            e.Handled = true;
            e.SuppressKeyPress = true;
            var handler = RecallLeft;
            if (handler != null) handler(this, EventArgs.Empty);
            return;
        }
        if (e.KeyCode == Keys.Right)
        {
            e.Handled = true;
            e.SuppressKeyPress = true;
            var handler = RecallRight;
            if (handler != null) handler(this, EventArgs.Empty);
            return;
        }
        base.OnKeyDown(e);
    }
}

public sealed class NativeRecallForm : Form
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
    private readonly int episodeCount;
    private readonly string tracePath;
    private readonly Label status;
    private readonly NativeRecallButton previousButton;
    private readonly NativeRecallButton nextButton;
    private readonly Button closeButton;
    private int index;

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

    private string EpisodeMessage(int episodeIndex)
    {
        int offset = episodeIndex * 4;
        byte current = history[offset + 0];
        byte goal = history[offset + 1];
        byte target = history[offset + 2];
        byte result = history[offset + 3];

        foreach (byte x in new[] { current, goal, target, result })
            if (Array.IndexOf(states, x) < 0)
                throw new Exception("episodic history contains relation outside native domain");

        return "Episode " + (episodeIndex + 1) + " of " + episodeCount
            + ": current " + Role(current)
            + ", goal " + Role(goal)
            + ", target " + Role(target)
            + ", result " + Role(result);
    }

    private void Project(string carrier)
    {
        string message = EpisodeMessage(index);
        status.Text = message;
        status.AccessibleName = message;

        previousButton.Enabled = index > 0;
        nextButton.Enabled = index < episodeCount - 1;

        int running = nvdaController_testIfRunning();
        if (running != 0) throw new Exception("NVDA not available, error=" + running);
        int speech = nvdaController_speakText(message);
        if (speech != 0) throw new Exception("NVDA speech failed, error=" + speech);
        int braille = nvdaController_brailleMessage(message);
        if (braille != 0) throw new Exception("NVDA braille failed, error=" + braille);

        File.AppendAllText(tracePath,
            "carrier=" + carrier
            + ";episode-index=" + index
            + ";message=" + message
            + ";visible-text=" + status.Text
            + ";accessible-name=" + status.AccessibleName
            + ";speech-message=" + message
            + ";braille-message=" + message
            + ";nvda-running=" + running
            + ";nvda-speech=" + speech
            + ";nvda-braille=" + braille
            + Environment.NewLine);
    }

    private void Previous(string carrier)
    {
        if (index <= 0) return;
        index--;
        Project(carrier);
    }

    private void Next(string carrier)
    {
        if (index >= episodeCount - 1) return;
        index++;
        Project(carrier);
    }

    protected override bool ProcessDialogKey(Keys keyData)
    {
        if (keyData == Keys.Left)
        {
            Previous("keyboard-left");
            return true;
        }
        if (keyData == Keys.Right)
        {
            Next("keyboard-right");
            return true;
        }
        return base.ProcessDialogKey(keyData);
    }

    protected override bool ProcessCmdKey(ref Message msg, Keys keyData)
    {
        if (keyData == Keys.N)
        {
            Next("keyboard-n");
            return true;
        }
        if (keyData == Keys.P)
        {
            Previous("keyboard-p");
            return true;
        }
        if (keyData == Keys.Enter && nextButton != null && nextButton.Focused && nextButton.Enabled)
        {
            Next("keyboard-next");
            return true;
        }
        if (keyData == Keys.Enter && previousButton != null && previousButton.Focused && previousButton.Enabled)
        {
            Previous("keyboard-previous");
            return true;
        }
        if (keyData == Keys.Left)
        {
            Previous("keyboard-left");
            return true;
        }
        if (keyData == Keys.Right)
        {
            Next("keyboard-right");
            return true;
        }
        return base.ProcessCmdKey(ref msg, keyData);
    }

    public NativeRecallForm(string tablePath, string historyPath, string tracePath)
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

        history = File.ReadAllBytes(historyPath);
        if (history.Length == 0 || history.Length % 4 != 0)
            throw new Exception("native episodic history framing mismatch");
        episodeCount = history.Length / 4;
        index = 0;

        Text = "Native Recall V1";
        AccessibleName = "Native Recall V1";
        AccessibleDescription = "Accessible read-only recall of native episodic memory";
        StartPosition = FormStartPosition.CenterScreen;
        ClientSize = new Size(820, 250);

        status = new Label();
        status.Name = "NativeRecallStatus";
        status.AutoSize = false;
        status.Location = new Point(24, 24);
        status.Size = new Size(760, 70);
        status.Font = new Font(SystemFonts.MessageBoxFont.FontFamily, 12.0f);
        status.AccessibleDescription = "Current recalled native episode";
        status.AccessibleRole = AccessibleRole.StaticText;
        Controls.Add(status);

        previousButton = new NativeRecallButton();
        previousButton.Text = "Previous episode";
        previousButton.AccessibleName = "Previous native episode";
        previousButton.AccessibleDescription = "Recall the previous native episode";
        previousButton.AccessibleRole = AccessibleRole.PushButton;
        previousButton.Location = new Point(24, 125);
        previousButton.Size = new Size(220, 46);
        previousButton.TabIndex = 0;
        previousButton.Click += delegate { Previous("assistive-previous"); };
        previousButton.RecallLeft += delegate { Previous("keyboard-left"); };
        previousButton.RecallRight += delegate { Next("keyboard-right"); };
        Controls.Add(previousButton);

        nextButton = new NativeRecallButton();
        nextButton.Text = "Next episode";
        nextButton.AccessibleName = "Next native episode";
        nextButton.AccessibleDescription = "Recall the next native episode";
        nextButton.AccessibleRole = AccessibleRole.PushButton;
        nextButton.Location = new Point(264, 125);
        nextButton.Size = new Size(220, 46);
        nextButton.TabIndex = 1;
        nextButton.Click += delegate { Next("assistive-next"); };
        nextButton.RecallLeft += delegate { Previous("keyboard-left"); };
        nextButton.RecallRight += delegate { Next("keyboard-right"); };
        Controls.Add(nextButton);

        closeButton = new Button();
        closeButton.Text = "Close recall";
        closeButton.AccessibleName = "Close native recall";
        closeButton.AccessibleDescription = "Close the read-only native episodic recall surface";
        closeButton.AccessibleRole = AccessibleRole.PushButton;
        closeButton.Location = new Point(504, 125);
        closeButton.Size = new Size(220, 46);
        closeButton.TabIndex = 2;
        closeButton.Click += delegate { Close(); };
        Controls.Add(closeButton);

        File.WriteAllText(tracePath,
            "NATIVE-RECALL-V1" + Environment.NewLine
            + "episodes=" + episodeCount + Environment.NewLine
            + "read-only=true" + Environment.NewLine);

        Shown += delegate
        {
            nextButton.Focus();
            Project("startup");
        };

        var timer = new Timer();
        timer.Interval = 60000;
        timer.Tick += delegate { Close(); };
        timer.Start();
    }
}

public static class NativeRecallApp
{
    [STAThread]
    public static void Main(string[] args)
    {
        if (args.Length != 3)
            throw new ArgumentException("expected relation table history path trace path");
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.Run(new NativeRecallForm(args[0], args[1], args[2]));
    }
}
