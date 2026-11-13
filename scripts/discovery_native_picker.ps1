# Native Windows common-folder dialog acceptance, limited to the test app PID.
# No mock return value and no system permission/configuration changes.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$pickerPid = [int]$env:DEVFORGE_PICKER_PID
$action = $env:DEVFORGE_PICKER_ACTION
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class PickerNativeButton {
    [DllImport("user32.dll", SetLastError=true)]
    public static extern IntPtr SendMessageTimeout(IntPtr h, uint m, UIntPtr w, IntPtr l, uint flags, uint timeout, out UIntPtr result);
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr h, out uint process);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h, int id);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
}
'@
if ($action -eq 'tab') {
    $handle = (Get-Process -Id $pickerPid).MainWindowHandle
    [void][PickerNativeButton]::SetForegroundWindow($handle)
    Start-Sleep -Milliseconds 150
    [uint32]$owner = 0
    [void][PickerNativeButton]::GetWindowThreadProcessId([PickerNativeButton]::GetForegroundWindow(), [ref]$owner)
    if ($owner -ne $pickerPid) { throw 'Test app is not the foreground keyboard target' }
    [PickerNativeButton]::keybd_event(9, 0, 0, [UIntPtr]::Zero)
    [PickerNativeButton]::keybd_event(9, 0, 2, [UIntPtr]::Zero)
    @{ native_keyboard_tab = $true; pid_matched = $true } | ConvertTo-Json -Compress
    exit 0
}
$condition = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $pickerPid)),
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, '#32770'))
)
$deadline = [DateTime]::UtcNow.AddSeconds(20)
$dialog = $null
while ([DateTime]::UtcNow -lt $deadline -and $null -eq $dialog) {
    $dialogs = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
    foreach ($candidate in $dialogs) {
        $candidateHandle = [IntPtr]$candidate.Current.NativeWindowHandle
        if ($candidateHandle -ne [IntPtr]::Zero -and [PickerNativeButton]::IsWindowVisible($candidateHandle)) { $dialog = $candidate; break }
    }
    if ($null -eq $dialog) { Start-Sleep -Milliseconds 100 }
}
if ($null -eq $dialog) { throw 'Native folder dialog for the test PID was not found' }
if ($action -eq 'select') {
    $id = '1'
} elseif ($action -eq 'cancel') {
    $id = '2'
} else { throw 'Unknown native picker test action' }
# Common-dialog IDs are Win32 control IDs. Hosted UIA providers may omit the
# Button role/pattern altogether; operate the real HWND control directly.
$dialogHandle = [IntPtr]$dialog.Current.NativeWindowHandle
$handle = [IntPtr]::Zero
$deadline = [DateTime]::UtcNow.AddSeconds(5)
do {
    $handle = [PickerNativeButton]::GetDlgItem($dialogHandle, [int]$id)
    if ($handle -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 100 }
} while ($handle -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
$class = New-Object System.Text.StringBuilder 256
[void][PickerNativeButton]::GetClassName($handle, $class, 256)
[uint32]$owner = 0
[void][PickerNativeButton]::GetWindowThreadProcessId($handle, [ref]$owner)
if ($handle -eq [IntPtr]::Zero -or $owner -ne $pickerPid -or $class.ToString() -ne 'Button') {
    $controls = $dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    $summary = @($controls | Select-Object -First 80 | ForEach-Object { @{ id=$_.Current.AutomationId; role=$_.Current.ControlType.ProgrammaticName; class=$_.Current.ClassName; name=$_.Current.Name } }) | ConvertTo-Json -Compress
    throw "Real native button $id unavailable (class=$class, ownPID=$($owner -eq $pickerPid)); own-dialog controls=$summary"
}
[UIntPtr]$result = [UIntPtr]::Zero
if ([PickerNativeButton]::SendMessageTimeout($handle, 0x00F5, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 5000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Native button press failed' }
@{ native_dialog=$true; pid_matched=$true; action=$action; control=$id; native_class=$class.ToString(); method='BM_CLICK actual HWND button' } | ConvertTo-Json -Compress
