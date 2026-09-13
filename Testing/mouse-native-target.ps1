param([Parameter(Mandatory=$true)][string]$Probe)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Windows.Forms;
public class MouseClickTarget : Control {
    public int LeftDown, RightDown, MiddleDown, Ups, Doubles;
    public MouseClickTarget() {
        SetStyle(ControlStyles.StandardClick | ControlStyles.StandardDoubleClick, true);
        BackColor=Color.LightSkyBlue;
        MouseDown += delegate(object sender, MouseEventArgs e) { if(e.Button==MouseButtons.Left) LeftDown++; if(e.Button==MouseButtons.Right) RightDown++; if(e.Button==MouseButtons.Middle) MiddleDown++; };
        MouseUp += delegate { Ups++; };
        MouseDoubleClick += delegate { Doubles++; };
    }
}
public class MouseNativeTarget : Form {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool GetPhysicalCursorPos(out Point point);
    readonly MouseClickTarget clicks = new MouseClickTarget();
    readonly TextBox editor = new TextBox();
    readonly TextBox content = new TextBox();
    readonly Timer timer = new Timer();
    readonly string probe;
    readonly string[] actions = { "click left", "click right", "click middle", "click double_left", "move right 37", "move down 53", "move left 37", "move up 53", "scroll down 1", "scroll up 1", "scroll down 5", "scroll up 5" };
    public readonly List<string> Results = new List<string>();
    public bool Passed;
    int stage, left, right, middle, ups, doubles, line, smallDelta;
    bool pending;
    Point pointer;
    int FirstLine { get { return SendMessage(content.Handle, 0x00CE, IntPtr.Zero, IntPtr.Zero).ToInt32(); } }
    public MouseNativeTarget(string path) {
        probe=path; Text="SayAll isolated mouse verification"; Size=new Size(740,680);
        StartPosition=FormStartPosition.CenterScreen;
        editor.SetBounds(20,20,670,30); editor.Text="Focus check";
        clicks.SetBounds(20,70,670,130);
        content.SetBounds(20,220,670,370); content.Multiline=true; content.ScrollBars=ScrollBars.Vertical;
        for(int i=0;i<250;i++) content.AppendText("Line " + i + Environment.NewLine);
        Controls.Add(editor); Controls.Add(clicks); Controls.Add(content);
        timer.Tick += Tick;
        Shown += delegate {
            content.SelectionStart=0; content.ScrollToCaret();
            SendMessage(content.Handle, 0x00B6, IntPtr.Zero, new IntPtr(30));
            editor.Focus(); Cursor.Position=clicks.PointToScreen(new Point(200,70));
            timer.Interval=5500; timer.Start();
        };
    }
    void Inject(string action) {
        using(var process=Process.Start(new ProcessStartInfo(probe,action) { UseShellExecute=false, CreateNoWindow=true, RedirectStandardOutput=true, RedirectStandardError=true })) {
            if(!process.WaitForExit(5000) || process.ExitCode!=0) throw new Exception("probe failed: " + process.StandardError.ReadToEnd());
        }
    }
    void Check(bool condition, string message) { if(!condition) throw new Exception(message); }
    void Tick(object sender, EventArgs e) {
        try {
            if(!pending) {
                Check(ContainsFocus, "test window lost focus; refusing to inject");
                if(stage<4) Cursor.Position=clicks.PointToScreen(new Point(200,70));
                if(stage==4) { Cursor.Position=clicks.PointToScreen(new Point(200,70)); editor.Focus(); }
                if(stage>=8) { Cursor.Position=content.PointToScreen(new Point(200,150)); editor.Focus(); }
                GetPhysicalCursorPos(out pointer);
                left=clicks.LeftDown; right=clicks.RightDown; middle=clicks.MiddleDown; ups=clicks.Ups; doubles=clicks.Doubles; line=FirstLine;
                Inject(actions[stage]); pending=true; timer.Interval=1000; return;
            }
            Point after; GetPhysicalCursorPos(out after);
            if(stage<4) {
                Check(after==pointer, "click moved pointer dx="+(after.X-pointer.X)+" dy="+(after.Y-pointer.Y)+" left_down="+(clicks.LeftDown-left)+" ups="+(clicks.Ups-ups));
                Check(clicks.Ups-ups==(stage==3 ? 2 : 1), "mouse button UP count");
                Check(clicks.LeftDown-left==(stage==0 ? 1 : stage==3 ? 2 : 0), "left DOWN count");
                Check(clicks.RightDown-right==(stage==1 ? 1 : 0), "right DOWN count");
                Check(clicks.MiddleDown-middle==(stage==2 ? 1 : 0), "middle DOWN count");
                Check(clicks.Doubles-doubles==(stage==3 ? 1 : 0), "native double click recognition");
            } else if(stage<8) {
                int dx=stage==4 ? 37 : stage==6 ? -37 : 0;
                int dy=stage==5 ? 53 : stage==7 ? -53 : 0;
                Check(after.X-pointer.X==dx && after.Y-pointer.Y==dy, "physical pixel distance expected="+dx+","+dy+" actual="+(after.X-pointer.X)+","+(after.Y-pointer.Y));
                Check(clicks.Ups==ups && clicks.LeftDown==left && clicks.RightDown==right, "move unexpectedly clicked");
                Check(ActiveControl==editor, "move changed input focus");
            } else {
                Check(after==pointer && ActiveControl==editor, "scroll changed pointer/focus");
                int delta=FirstLine-line;
                if(stage==8) { Check(delta>0, "scroll down"); smallDelta=delta; }
                if(stage==9) Check(delta==-smallDelta, "scroll up");
                if(stage==10) Check(delta==smallDelta*5, "five-notch scroll down");
                if(stage==11) Check(delta==-smallDelta*5, "five-notch scroll up");
            }
            Results.Add("passed: " + actions[stage] + " scroll_lines=" + (FirstLine-line));
            stage++; pending=false;
            if(stage==actions.Length) { Passed=true; timer.Stop(); Close(); }
        } catch(Exception error) { Results.Add("failed: " + actions[stage] + " " + error.Message); timer.Stop(); Close(); }
    }
}
"@
[MouseNativeTarget]::SetProcessDPIAware() | Out-Null
$form=New-Object MouseNativeTarget((Resolve-Path -LiteralPath $Probe).Path)
[System.Windows.Forms.Application]::Run($form)
$form.Results
if(-not $form.Passed) { exit 1 }
