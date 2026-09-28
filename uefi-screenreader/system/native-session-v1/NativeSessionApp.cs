using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Forms;

public sealed class NativeSessionForm : Form
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
    private readonly string memoryPath;
    private readonly string tracePath;
    private readonly string mode;
    private readonly Label status;
    private readonly Button primaryButton;
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

    private void ProjectNvda(string message)
    {
        int running = nvdaController_testIfRunning();
        if (running != 0) throw new Exception("NVDA not available, error=" + running);

        int speech = nvdaController_speakText(message);
        if (speech != 0) throw new Exception("NVDA speech failed, error=" + speech);

        int braille = nvdaController_brailleMessage(message);
        if (braille != 0) throw new Exception("NVDA braille failed, error=" + braille);

        File.AppendAllText(tracePath,
            "projection-message=" + message
            + ";visible-text=" + status.Text
            + ";accessible-name=" + status.AccessibleName
            + ";speech-message=" + message
            + ";braille-message=" + message
            + ";nvda-running=" + running
            + ";nvda-speech=" + speech
            + ";nvda-braille=" + braille
            + Environment.NewLine);
    }

    protected override bool ProcessCmdKey(ref Message msg, Keys keyData)
    {
        if (mode == "fresh" && keyData == Keys.Enter && primaryButton != null && primaryButton.Focused)
        {
            PersistFromInput("keyboard-enter");
            return true;
        }
        return base.ProcessCmdKey(ref msg, keyData);
    }

    private void PersistFromInput(string externalCarrier)
    {
        if (mode != "fresh") throw new Exception("input is only valid in fresh mode");
        if (acceptedInputs != 0) throw new Exception("fresh session accepts exactly one external input");

        byte observed = current;
        byte action = FindInverse(observed);
        byte actual = Compose(observed, action);
        if (actual != identity) throw new Exception("native action missed structural goal");

        current = actual;
        acceptedInputs++;
        ApplyHumanProjection();

        Directory.CreateDirectory(Path.GetDirectoryName(memoryPath) ?? ".");
        File.WriteAllBytes(memoryPath, new byte[] { current });

        string message = HumanProjection(current);
        ProjectNvda(message);

        File.AppendAllText(tracePath,
            "external-carrier=" + externalCarrier
            + ";observed-role=" + Role(observed)
            + ";normalized-action-role=" + Role(action)
            + ";actual-role=" + Role(actual)
            + ";persisted-role=" + Role(current)
            + ";memory-length=1"
            + Environment.NewLine);

        BeginInvoke((MethodInvoker)delegate { Close(); });
    }

    private void LoadResumeState()
    {
        if (!File.Exists(memoryPath)) throw new Exception("native session memory missing");
        byte[] memory = File.ReadAllBytes(memoryPath);
        if (memory.Length != 1) throw new Exception("native session memory length mismatch");
        if (Array.IndexOf(states, memory[0]) < 0) throw new Exception("persisted state outside native relation carrier");

        current = memory[0];
        ApplyHumanProjection();
        string message = HumanProjection(current);

        File.AppendAllText(tracePath,
            "resume-loaded-role=" + Role(current)
            + ";memory-length=" + memory.Length
            + Environment.NewLine);

        ProjectNvda(message);
    }

    public NativeSessionForm(string tablePath, string memoryPath, string tracePath, string mode)
    {
        this.memoryPath = memoryPath;
        this.tracePath = tracePath;
        this.mode = mode;

        if (mode != "fresh" && mode != "resume")
            throw new ArgumentException("mode must be fresh or resume");

        table = File.ReadAllBytes(tablePath);
        if (table.Length != 4) throw new Exception("native relation table length mismatch");

        var unique = new List<byte>();
        for (int i = 0; i < table.Length; i++)
            if (!unique.Contains(table[i])) unique.Add(table[i]);
        states = unique.ToArray();
        if (states.Length != 2) throw new Exception("expected exactly two native relation carriers");

        identity = FindIdentity();
        exchange = states[0] == identity ? states[1] : states[0];

        Text = mode == "fresh" ? "Native Session V1 Fresh" : "Native Session V1 Resume";
        AccessibleName = Text;
        AccessibleDescription = "Accessible native state persistence and restoration witness";
        StartPosition = FormStartPosition.CenterScreen;
        ClientSize = new Size(600, 200);

        status = new Label();
        status.Name = "NativeSessionStatus";
        status.AutoSize = false;
        status.Location = new Point(24, 24);
        status.Size = new Size(540, 44);
        status.Font = new Font(SystemFonts.MessageBoxFont.FontFamily, 14.0f);
        status.AccessibleDescription = "Current native session state";
        status.AccessibleRole = AccessibleRole.StaticText;
        Controls.Add(status);

        primaryButton = new Button();
        primaryButton.Location = new Point(24, 94);
        primaryButton.Size = new Size(320, 44);
        primaryButton.TabIndex = 0;
        primaryButton.TabStop = true;
        primaryButton.AccessibleRole = AccessibleRole.PushButton;
        Controls.Add(primaryButton);

        File.WriteAllText(tracePath,
            "NATIVE-SESSION-V1" + Environment.NewLine
            + "mode=" + mode + Environment.NewLine);

        if (mode == "fresh")
        {
            current = exchange;
            ApplyHumanProjection();
            primaryButton.Text = "Persist native state";
            primaryButton.AccessibleName = "Persist native state";
            primaryButton.AccessibleDescription = "Apply the native action and persist the resulting state";
            primaryButton.Click += delegate { PersistFromInput("assistive-invoke"); };

            File.AppendAllText(tracePath,
                "initial-role=" + Role(current) + Environment.NewLine
                + "initial-visible=" + status.Text + Environment.NewLine
                + "initial-accessible=" + status.AccessibleName + Environment.NewLine);

            Shown += delegate { primaryButton.Focus(); };
        }
        else
        {
            primaryButton.Text = "Close restored session";
            primaryButton.AccessibleName = "Close restored session";
            primaryButton.AccessibleDescription = "Close the restored native session witness";
            primaryButton.Click += delegate { Close(); };

            LoadResumeState();
            Shown += delegate { primaryButton.Focus(); };
        }

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

public static class NativeSessionApp
{
    [STAThread]
    public static void Main(string[] args)
    {
        if (args.Length != 4)
            throw new ArgumentException("expected relation table path memory path trace path mode");
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.Run(new NativeSessionForm(args[0], args[1], args[2], args[3]));
    }
}
