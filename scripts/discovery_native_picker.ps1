# Native Windows common-folder dialog acceptance, limited to the test app PID.
# No mock return value and no system permission/configuration changes.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$pickerPid = [int]$env:DEVFORGE_PICKER_PID
$action = $env:DEVFORGE_PICKER_ACTION
$condition = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $pickerPid)),
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, '#32770'))
)
$deadline = [DateTime]::UtcNow.AddSeconds(20)
$dialog = $null
while ([DateTime]::UtcNow -lt $deadline -and $null -eq $dialog) {
    $dialog = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
    if ($null -eq $dialog) { Start-Sleep -Milliseconds 100 }
}
if ($null -eq $dialog) { throw 'Native folder dialog for the test PID was not found' }
if ($action -eq 'select') {
    $id = '1'
} elseif ($action -eq 'cancel') {
    $id = '2'
} else { throw 'Unknown native picker test action' }
$buttonCondition = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, $id)),
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button))
)
$button = $dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $buttonCondition)
if ($null -eq $button) { throw "Native picker button $id was not found" }
$pattern = $null
if ($button.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
    $pattern.Invoke()
    $method = 'InvokePattern'
} elseif ($button.TryGetCurrentPattern([System.Windows.Automation.LegacyIAccessiblePattern]::Pattern, [ref]$pattern)) {
    $pattern.DoDefaultAction()
    $method = 'LegacyIAccessiblePattern'
} else {
    # A real native HWND button may have no UIA provider on hosted runners.
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class PickerNativeButton {
    [DllImport("user32.dll", SetLastError=true)]
    public static extern IntPtr SendMessageTimeout(IntPtr h, uint m, UIntPtr w, IntPtr l, uint flags, uint timeout, out UIntPtr result);
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr h, out uint process);
}
'@
    $handle = [IntPtr]$button.Current.NativeWindowHandle
    [uint32]$owner = 0
    [void][PickerNativeButton]::GetWindowThreadProcessId($handle, [ref]$owner)
    if ($handle -eq [IntPtr]::Zero -or $owner -ne $pickerPid) { throw 'Native button handle does not belong to test PID' }
    [UIntPtr]$result = [UIntPtr]::Zero
    if ([PickerNativeButton]::SendMessageTimeout($handle, 0x00F5, [UIntPtr]::Zero, [IntPtr]::Zero, 2, 5000, [ref]$result) -eq [IntPtr]::Zero) { throw 'Native button press failed' }
    $method = 'BM_CLICK'
}
@{ native_dialog = $true; pid_matched = $true; action = $action; control = $id; method = $method } | ConvertTo-Json -Compress
