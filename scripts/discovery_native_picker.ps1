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
    $shell = New-Object -ComObject WScript.Shell
    if (-not $shell.AppActivate($pickerPid)) { throw 'Could not activate the test picker' }
    $shell.SendKeys('%d')
    Start-Sleep -Milliseconds 200
    $address = [System.Windows.Automation.AutomationElement]::FocusedElement
    if ($address.Current.ProcessId -ne $pickerPid) { throw 'Address focus is outside the test process' }
    $value = $address.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    $value.SetValue($env:DEVFORGE_PICKER_PATH)
    $shell.SendKeys('{ENTER}')
    Start-Sleep -Milliseconds 400
    $id = '1'
} elseif ($action -eq 'cancel') {
    $id = '2'
} else { throw 'Unknown native picker test action' }
$buttonCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, $id)
$button = $dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $buttonCondition)
if ($null -eq $button) { throw "Native picker button $id was not found" }
$invoke = $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
$invoke.Invoke()
@{ native_dialog = $true; pid_matched = $true; action = $action; control = $id } | ConvertTo-Json -Compress
