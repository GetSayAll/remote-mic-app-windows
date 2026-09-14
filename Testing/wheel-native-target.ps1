param([Parameter(Mandatory=$true)][string]$Probe)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing -TypeDefinition @"
using System;
using System.Diagnostics;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Windows.Forms;
public class WheelNativeTarget : Form {
    [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    readonly TextBox editor = new TextBox();
    readonly TextBox content = new TextBox();
    readonly Timer timer = new Timer();
    readonly string probe;
    int stage, before, downLine;
    Point pointer;
    public string Result = "failed: incomplete";
    int FirstLine { get { return SendMessage(content.Handle, 0x00CE, IntPtr.Zero, IntPtr.Zero).ToInt32(); } }
    public WheelNativeTarget(string path) {
        probe = path; Text = "SayAll native wheel verification"; Size = new Size(720,580);
        StartPosition = FormStartPosition.CenterScreen;
        editor.SetBounds(20,20,640,30); editor.Text = "Focus must remain here";
        content.SetBounds(20,70,640,420); content.Multiline = true; content.ScrollBars = ScrollBars.Vertical;
        for (int i=0;i<200;i++) content.AppendText("Line " + i + Environment.NewLine);
        Controls.Add(editor); Controls.Add(content);
        timer.Tick += Tick;
        Shown += delegate {
            content.SelectionStart = 0; content.ScrollToCaret();
            SendMessage(content.Handle, 0x00B6, IntPtr.Zero, new IntPtr(30));
            editor.Focus();
            Cursor.Position = content.PointToScreen(new Point(200,150));
            pointer = Cursor.Position; before = FirstLine;
            timer.Interval = 5500; timer.Start();
        };
    }
    void Inject(string direction) {
        using (var process = Process.Start(new ProcessStartInfo(probe, direction + " 0") {
            UseShellExecute=false, CreateNoWindow=true, RedirectStandardOutput=true
        })) {
            if (!process.WaitForExit(5000) || process.ExitCode != 0) throw new Exception("probe failed");
        }
    }
    void Tick(object sender, EventArgs args) {
        try {
            if (stage == 0) { Inject("down"); stage=1; timer.Interval=1000; return; }
            if (stage == 1) {
                downLine=FirstLine;
                if (downLine <= before || ActiveControl != editor || Cursor.Position != pointer) throw new Exception("down/focus/pointer failed focus=" + (ActiveControl == editor) + " pointer=" + (Cursor.Position == pointer) + " expected=" + pointer + " actual=" + Cursor.Position);
                Inject("up"); stage=2; return;
            }
            int upLine=FirstLine;
            bool passed=upLine < downLine && ActiveControl == editor && Cursor.Position == pointer;
            Result=String.Format("result={0} before={1} down={2} up={3} focus_preserved={4} pointer_preserved={5}", passed ? "passed" : "failed", before, downLine, upLine, ActiveControl==editor, Cursor.Position==pointer);
            timer.Stop(); Close();
        } catch (Exception error) { Result="failed: " + error.Message + " before=" + before + " current=" + FirstLine; timer.Stop(); Close(); }
    }
}
"@
$form = New-Object WheelNativeTarget((Resolve-Path -LiteralPath $Probe).Path)
[System.Windows.Forms.Application]::Run($form)
Write-Output $form.Result
if ($form.Result -notlike "result=passed*") { exit 1 }
