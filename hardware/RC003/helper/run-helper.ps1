# RC003 enhanced-capture helper -- launcher (see run-helper*.cmd for right-click entry).
#
# Why a launcher exists: the helper must run ELEVATED (the target WUDFHost.exe
# lives in session 0, so a normal token cannot inject into it), and an agent
# cannot trigger an elevation prompt on the user's behalf. So the user starts
# this through a .cmd and answers the UAC prompt.
#
# Modes (in the order you should actually use them):
#   selftest  - no elevation, no device, no injection. 42 built-in checks.
#   dryrun    - NO elevation needed (every check is read-only); locates the
#               RC003 host, checks host exclusivity, verifies the Gadget hash,
#               and reports what a real run would do with the runtime directory.
#               Does NOT inject and does NOT listen.
#   observe   - elevated; injects and reports key edges, but NEVER clears a
#               report. Zero regression risk -- use this to confirm the three
#               keys actually reach us before enabling interception.
#   run       - elevated; injects and clears ONLY the three target usages
#               (0x00F1 / 0x0080 / 0x0081). OK / Home / arrows keep using the
#               native Windows path.
#
# -Canary <usage> (acceptance only; used by run-helper-canary.cmd):
#   On RC003 the three target keys produce NO Windows events at all, because
#   kbdhid drops those three usages. So "cleared" and "not cleared" are
#   externally INDISTINGUISHABLE: volume does not change either way, no
#   character appears. The classic acceptance criterion ("no native action")
#   is therefore always true on RC003 -- it has no discriminating power.
#   Passing -Canary 0x4A ALSO clears the Home key (which Windows CAN handle),
#   which turns clearing into something you can see with your own eyes:
#     * Home works before the run            -> baseline (the key is reachable)
#     * while armed                          -> Home does nothing (clearing works)
#     * after exit / lease expiry / Ctrl+C   -> Home works again (fail-open)
#   It stays bounded by -Duration and by the 2 s agent-side lease.
#
# Running it a second time (2026-09-23 fix; read this if a re-run ever fails):
#   A successful injection makes the host map the runtime-dir Gadget DLL, and
#   the host can live for many hours -- so the file is locked against overwrite
#   (`os error 32`). Before this fix every re-run died on that copy, before it
#   ever reached injection. The helper now enumerates the host's modules first:
#     * our tap is there and the token matches -> attach; no copy, no injection
#     * our tap is there but the token cannot match (pre-fix generation)
#       -> [STALE-TAP] with the exact clean-up steps (exit 13)
#     * no tap -> copy (reusing the file when its SHA-256 already matches) + inject
#   The token is now PERSISTED in the runtime dir (session.token) instead of
#   being regenerated per run, which is what makes "attach" possible at all.
#   --new-token forces a new one; --attach-only never injects.
#   --await-hello <sec> (default 30) turns "injected but no session" from a
#   silent hang into [NO-HELLO] + exit code 12.
#
# Safety properties of every elevated mode:
#   * no kernel driver, no TESTSIGNING, no Secure Boot change, no reboot,
#     no registry write, no scheduled task
#   * a lease (2 s, renewed every 500 ms) makes the agent stop clearing by
#     itself if the helper dies -- keys return to their native behaviour
#   * -Duration bounds the run so the hook cannot be left behind by accident.
#     NOTE: until 2026-09-23 this bound did NOT fire while an agent was
#     connected (the read loop blocked, so the deadline was never checked).
#     Fixed; see the helper header and docs/investigations/.
#   * Ctrl+C or closing the window now sends `disarm` before exiting
#     (a console control handler was added for this; before that the process
#     was hard-killed and `disarm` was never sent).
#
# Keep this file ASCII-only to avoid encoding corruption on Windows.

param(
    [ValidateSet('selftest', 'dryrun', 'observe', 'run')]
    [string]$Mode = 'run',
    [int]$Duration = 300,
    # Acceptance-only. Extra HID usages to clear besides the three targets;
    # hex, comma separated (e.g. '0x4A'). Empty = disabled. See the header.
    [string]$Canary = ''
)

$ErrorActionPreference = 'Stop'

# The helper is a Rust binary: it writes UTF-8 bytes to the console. Windows'
# default console code page on this machine is GBK (CP936), so every Chinese
# message comes out as mojibake -- which is bad here, because all the operating
# instructions are Chinese. Switching the console to UTF-8 for the duration of
# this launcher fixes both the displayed text and any piped/redirected capture.
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }

$root   = $PSScriptRoot
$exe    = Join-Path $root 'target\release\sayall-helper.exe'
$gadget = Join-Path $root 'vendor\frida-gadget.dll'
$log    = Join-Path (Split-Path $root -Parent) 'evidence\helper-run.log'

Write-Host ''
Write-Host ('=== RC003 helper :: mode=' + $Mode + ' ===') -ForegroundColor Cyan
Write-Host 'No kernel driver / no reboot / no system setting is changed.'
Write-Host ''

if (-not (Test-Path $exe)) {
    Write-Host ('helper not built: ' + $exe) -ForegroundColor Red
    Write-Host 'Build it first (from the helper directory), then re-run this launcher:'
    Write-Host '    cargo build --release'
    exit 2
}

# selftest does not inject, so it must stay runnable from a normal shell.
if ($Mode -ne 'selftest' -and -not (Test-Path $gadget)) {
    Write-Host ('frida-gadget.dll missing: ' + $gadget) -ForegroundColor Red
    Write-Host 'Fetch and verify it (version + SHA-256 come from vendor\frida-gadget.lock.json):'
    Write-Host ('    ' + 'python ' + (Join-Path $root 'vendor\fetch_frida_gadget.py'))
    exit 2
}

# Only the injecting modes need elevation. dryrun is read-only, so requiring
# UAC for it would hide exactly the failures it exists to catch -- the
# 2026-09-23 registry bug ("HostPid unreadable") would have surfaced without a
# single UAC prompt had dryrun not been gated too.
if ($Mode -eq 'observe' -or $Mode -eq 'run') {
    $admin = ([Security.Principal.WindowsPrincipal] `
              [Security.Principal.WindowsIdentity]::GetCurrent()
             ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if (-not $admin) {
        Write-Host 'not elevated -- this mode must run as administrator.' -ForegroundColor Red
        Write-Host 'Close this window and use the right-click entry instead:'
        Write-Host '    right-click run-helper-observe.cmd  ->  observe (no interception)'
        Write-Host '    right-click run-helper.cmd          ->  interception ON'
        Write-Host ('    (read-only checks, no elevation: ' + $exe + ' --selftest / --dry-run)')
        exit 3
    }
    Write-Host 'elevated: yes'
} else {
    Write-Host 'elevated: not required for this mode'
}

$helperArgs = @('--gadget', $gadget, '--log', $log)

switch ($Mode) {
    'selftest' {
        # --selftest ignores the other flags; keep the call explicit anyway.
        $helperArgs = @('--selftest')
    }
    'dryrun' {
        $helperArgs += '--dry-run'
    }
    'observe' {
        $helperArgs += @('--observe', '--duration', $Duration)
    }
    'run' {
        $helperArgs += @('--duration', $Duration)
    }
}

Write-Host ('log file: ' + $log)

# Canary is appended AFTER the mode switch: the selftest branch rebuilds
# helperArgs wholesale, so anything added earlier would silently disappear.
if ($Canary -ne '' -and ($Mode -eq 'observe' -or $Mode -eq 'run')) {
    $helperArgs += @('--canary-usage', $Canary)
    Write-Host ''
    Write-Host ('CANARY ENABLED -- these usages will ALSO be cleared: ' + $Canary) -ForegroundColor Yellow
    Write-Host 'Their native behaviour disappears while the helper is armed. That is the whole point:'
    Write-Host 'on RC003 it is the only way to SEE that clearing actually reaches the report.'
    Write-Host 'It comes back when the helper exits, the lease (2 s) expires, or you press Ctrl+C.'
}
Write-Host ''
switch ($Mode) {
    'observe' {
        Write-Host ('Now press the three keys on the RC003 remote. Nothing is intercepted:')
        Write-Host ('expect [EDGE] lines for back / volume_up / volume_down. Runs ' + $Duration + ' s.')
        Write-Host 'End early with Ctrl+C or by closing this window (a disarm is sent first).'
    }
    'run' {
        Write-Host ('Interception is ON for back / volume_up / volume_down only.')
        if ($Canary -ne '') {
            Write-Host ('CANARY: Home must now do NOTHING while armed (open Notepad and try it).') -ForegroundColor Yellow
            Write-Host 'Then press Home again AFTER this run ends -- it must work. That pair is the proof.'
        } else {
            Write-Host ('Press them, then also press OK / Home / arrows to confirm no regression.')
        }
        Write-Host ('Runs ' + $Duration + ' s, then exits and restores native behaviour.')
        Write-Host 'End early with Ctrl+C or by closing this window (a disarm is sent first).'
    }
    'dryrun' {
        Write-Host 'Locating the host, checking exclusivity and verifying the Gadget.'
        Write-Host 'No injection, no listening, and no elevation needed (double-click is fine).'
    }
    'selftest' {
        Write-Host 'Running the built-in self-test (no elevation, no device, no injection).'
    }
}
Write-Host ''

# Echo the exact command line. Without this, a bug in the argument-building
# switch above is invisible -- the helper would just run with the wrong flags
# and fail for a reason that looks unrelated (this actually happened once:
# a stale splat sent NO arguments, so every mode hit the elevation guard).
Write-Host ('command: ' + $exe + ' ' + ($helperArgs -join ' '))

& $exe @helperArgs
$code = $LASTEXITCODE

Write-Host ''
Write-Host ('exit code: ' + $code) -ForegroundColor Cyan
if ($Mode -eq 'run' -or $Mode -eq 'observe') {
    Write-Host 'Done. The hook is gone; keys are back to native behaviour.'
    if ($Canary -ne '') {
        Write-Host ('Canary ' + $Canary + ': TRY IT NOW -- it must work again. If it does not, that is') -ForegroundColor Yellow
        Write-Host 'a real finding: report it, then close the window / unplug the remote to recover.'
    }
}
Write-Host 'Log is already on disk; you can close this window.'
Start-Sleep -Seconds 20
exit $code
