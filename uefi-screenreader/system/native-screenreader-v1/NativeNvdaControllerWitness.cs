using System;
using System.Runtime.InteropServices;

public static class NativeNvdaControllerWitness
{
    [DllImport("nvdaControllerClient.dll")]
    private static extern int nvdaController_testIfRunning();

    [DllImport("nvdaControllerClient.dll", CharSet = CharSet.Unicode)]
    private static extern int nvdaController_speakText(string text);

    [DllImport("nvdaControllerClient.dll", CharSet = CharSet.Unicode)]
    private static extern int nvdaController_brailleMessage(string message);

    [DllImport("nvdaControllerClient.dll", CharSet = CharSet.Unicode)]
    private static extern int nvdaController_getProcessId(out uint processId);

    public static string Run(string semanticMessage)
    {
        if (string.IsNullOrWhiteSpace(semanticMessage))
            throw new ArgumentException("semantic message must not be empty");

        int running = nvdaController_testIfRunning();
        if (running != 0)
            throw new InvalidOperationException("NVDA controller reports not running, error=" + running);

        uint pid = 0;
        int pidResult = nvdaController_getProcessId(out pid);
        if (pidResult != 0 && pidResult != 1717)
            throw new InvalidOperationException("NVDA process-id query failed, error=" + pidResult);

        int speech = nvdaController_speakText(semanticMessage);
        if (speech != 0)
            throw new InvalidOperationException("NVDA speech projection failed, error=" + speech);

        int braille = nvdaController_brailleMessage(semanticMessage);
        if (braille != 0)
            throw new InvalidOperationException("NVDA braille projection failed, error=" + braille);

        return "NATIVE_NVDA_CONTROLLER=PASS"
            + ";TEST_IF_RUNNING=0"
            + ";SPEAK_TEXT=0"
            + ";BRAILLE_MESSAGE=0"
            + ";GET_PROCESS_ID=" + pidResult
            + ";NVDA_PID=" + pid;
    }
}
