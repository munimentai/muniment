$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$homePath = $env:MUNIMENT_FOLDER_PATH
$deadline = [DateTime]::UtcNow.AddSeconds([int]$env:MUNIMENT_FOLDER_WAIT_SECONDS)
$root = [System.Windows.Automation.AutomationElement]::RootElement
$children = [System.Windows.Automation.TreeScope]::Children
$descendants = [System.Windows.Automation.TreeScope]::Descendants
$all = [System.Windows.Automation.Condition]::TrueCondition

function Wait-PickerStep([string]$step) {
  if ([DateTime]::UtcNow -ge $deadline) { throw "The shell folder dialog timed out during $step." }
  Start-Sleep -Milliseconds 100
}

function Find-FolderDialog {
  $processIds = @(Get-Process -Name 'muniment-desktop' -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
  $matches = @($root.FindAll($children, $all) | Where-Object {
    $_.Current.ProcessId -in $processIds -and
    $_.Current.ClassName -eq '#32770' -and -not $_.Current.IsOffscreen
  })
  if ($matches.Count -gt 1) { throw 'Muniment has more than one shell dialog.' }
  if ($matches.Count -eq 1) { return $matches[0] }
}

try {
  $dialog = Find-FolderDialog
  while ($null -eq $dialog) {
    Wait-PickerStep 'the window search'
    $dialog = Find-FolderDialog
  }
  Write-Output "window: $($dialog.Current.Name). process: $($dialog.Current.ProcessId)"
  $handle = $dialog.Current.NativeWindowHandle
  $editCondition = [System.Windows.Automation.AndCondition]::new(
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '1152'),
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Edit)
  )
  $confirmCondition = [System.Windows.Automation.AndCondition]::new(
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '1'),
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)
  )
  $edit = $dialog.FindFirst($descendants, $editCondition)
  while ($null -eq $edit -or -not $edit.Current.IsEnabled -or $edit.Current.IsOffscreen) {
    Wait-PickerStep 'the Folder field search'
    $edit = $dialog.FindFirst($descendants, $editCondition)
  }
  $value = $edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
  $value.SetValue($homePath)
  if ($value.Current.Value -cne $homePath) { throw 'The Folder field did not keep the Home path.' }
  $confirm = $dialog.FindFirst($descendants, $confirmCondition)
  while ($null -eq $confirm -or -not $confirm.Current.IsEnabled -or $confirm.Current.IsOffscreen) {
    Wait-PickerStep 'the Select Folder button search'
    $confirm = $dialog.FindFirst($descendants, $confirmCondition)
  }
  $confirm.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  while ($null -ne ($root.FindAll($children, $all) | Where-Object {
    $_.Current.NativeWindowHandle -eq $handle -and -not $_.Current.IsOffscreen
  })) {
    Wait-PickerStep 'the window close'
  }
} catch {
  try {
    $root.FindAll($children, $all) | Where-Object { -not $_.Current.IsOffscreen } | ForEach-Object {
      Write-Output "visible window: $($_.Current.Name). class: $($_.Current.ClassName). process: $($_.Current.ProcessId)"
    }
  } catch {}
  throw
}
