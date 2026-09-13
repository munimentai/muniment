$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

# EnumWindows includes hidden windows that the UI Automation tree can omit.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace MunimentFolderPicker {
  public static class Desktop {
    public delegate bool EnumWindowProc(IntPtr handle, IntPtr parameter);
    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowProc callback, IntPtr parameter);
    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr handle, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetClassName(IntPtr handle, StringBuilder text, int capacity);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr handle, StringBuilder text, int capacity);
    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr handle);
    [DllImport("user32.dll")]
    private static extern bool GetWindowRect(IntPtr handle, out Rectangle rectangle);
    [DllImport("user32.dll")]
    private static extern IntPtr MonitorFromWindow(IntPtr handle, uint flags);

    [DllImport("user32.dll")]
    private static extern bool IsChild(IntPtr parent, IntPtr child);
    [DllImport("user32.dll")]
    private static extern bool IsWindowEnabled(IntPtr handle);
    [DllImport("user32.dll")]
    private static extern int GetDlgCtrlID(IntPtr handle);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "SendMessageTimeoutW")]
    private static extern IntPtr SendText(IntPtr handle, uint message, UIntPtr parameter, string text,
      uint flags, uint timeout, out UIntPtr result);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "SendMessageTimeoutW")]
    private static extern IntPtr ReadText(IntPtr handle, uint message, UIntPtr capacity, StringBuilder text,
      uint flags, uint timeout, out UIntPtr result);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "SendMessageTimeoutW")]
    private static extern IntPtr SendClick(IntPtr handle, uint message, UIntPtr parameter, IntPtr data,
      uint flags, uint timeout, out UIntPtr result);

    private static void CheckControl(IntPtr dialog, IntPtr control, int id, string expectedClass) {
      var name = new StringBuilder(256);
      GetClassName(control, name, name.Capacity);
      if (!IsChild(dialog, control) || GetDlgCtrlID(control) != id ||
          !String.Equals(name.ToString(), expectedClass, StringComparison.Ordinal) ||
          !IsWindowEnabled(control) || !IsWindowVisible(control))
        throw new InvalidOperationException("The native control does not match the ready dialog control.");
    }
    public static void SetFolder(IntPtr dialog, IntPtr control, string path) {
      CheckControl(dialog, control, 1152, "Edit");
      UIntPtr result;
      // WM_SETTEXT and WM_GETTEXT preserve the literal Unicode path and bound each cross-process call.
      if (SendText(control, 0x000C, UIntPtr.Zero, path, 0x23, 1000, out result) == IntPtr.Zero ||
          result == UIntPtr.Zero)
        throw new InvalidOperationException("The Folder field rejected WM_SETTEXT or did not respond within one second.");
      var text = new StringBuilder(path.Length + 2);
      if (ReadText(control, 0x000D, new UIntPtr((uint)text.Capacity), text, 0x23, 1000, out result) == IntPtr.Zero)
        throw new InvalidOperationException("The Folder field did not answer WM_GETTEXT within one second.");
      if (!String.Equals(text.ToString(), path, StringComparison.Ordinal))
        throw new InvalidOperationException("The Folder field did not keep the Home path.");
    }
    public static void ConfirmFolder(IntPtr dialog, IntPtr control) {
      CheckControl(dialog, control, 1, "Button");
      // BM_CLICK follows the button's focus behavior instead of only notifying the dialog.
      // Omit SMTO_ERRORONEXIT because a successful click can destroy the button.
      UIntPtr result;
      if (SendClick(control, 0x00F5, UIntPtr.Zero, IntPtr.Zero, 0x03, 1000, out result) == IntPtr.Zero)
        throw new InvalidOperationException("The Select Folder button did not answer BM_CLICK within one second.");
    }
    public static string FolderText(IntPtr dialog, IntPtr control) {
      CheckControl(dialog, control, 1152, "Edit");
      var text = new StringBuilder(32768);
      UIntPtr result;
      if (ReadText(control, 0x000D, new UIntPtr((uint)text.Capacity), text, 0x23, 1000, out result) == IntPtr.Zero)
        throw new InvalidOperationException("The Folder field did not answer WM_GETTEXT within one second.");
      return text.ToString();
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct Rectangle { public int Left, Top, Right, Bottom; }
    public class Window {
      public long Handle;
      public uint ProcessId;
      public string Class, Title;
      public bool Visible, Offscreen, RectangleAvailable;
      public Rectangle Rectangle;
    }
    public static Window[] Windows() {
      var windows = new List<Window>();
      EnumWindowProc callback = delegate(IntPtr handle, IntPtr parameter) {
        uint processId;
        GetWindowThreadProcessId(handle, out processId);
        var name = new StringBuilder(512);
        var title = new StringBuilder(32768);
        GetClassName(handle, name, name.Capacity);
        GetWindowText(handle, title, title.Capacity);
        Rectangle rectangle;
        bool available = GetWindowRect(handle, out rectangle);
        windows.Add(new Window {
          Handle = handle.ToInt64(), ProcessId = processId, Class = name.ToString(),
          Title = title.ToString(), Visible = IsWindowVisible(handle),
          Offscreen = MonitorFromWindow(handle, 0) == IntPtr.Zero,
          Rectangle = rectangle, RectangleAvailable = available
        });
        return true;
      };
      if (!EnumWindows(callback, IntPtr.Zero)) throw new InvalidOperationException("The desktop window query failed.");
      return windows.ToArray();
    }
  }
}
'@

$homePath = $env:MUNIMENT_FOLDER_PATH
$deadline = [DateTime]::UtcNow.AddSeconds([int]$env:MUNIMENT_FOLDER_WAIT_SECONDS)
$descendants = [System.Windows.Automation.TreeScope]::Descendants

function Write-PickerWindows {
  $appIds = @(Get-Process -Name 'muniment-desktop' -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
  $processes = @()
  try {
    $processes = @(Get-CimInstance Win32_Process -OperationTimeoutSec 5 -ErrorAction Stop)
  } catch {
    Write-Output "process tree error: $($_.Exception.Message)"
  }
  $family = @($appIds)
  do {
    $added = @($processes | Where-Object { $_.ParentProcessId -in $family -and $_.ProcessId -notin $family })
    $family += @($added | ForEach-Object { $_.ProcessId })
  } while ($added.Count -gt 0)
  $webviews = @($processes | Where-Object { $_.ProcessId -in $family -and $_.Name -ieq 'msedgewebview2.exe' })
  $processIds = @($appIds) + @($webviews | ForEach-Object { $_.ProcessId })
  Write-Output "app processes: $($appIds -join ', '). WebView2 processes: $(($webviews | ForEach-Object { $_.ProcessId }) -join ', ')"
  $windows = @([MunimentFolderPicker.Desktop]::Windows())
  foreach ($processId in $processIds) {
    $process = $processes | Where-Object { $_.ProcessId -eq $processId } | Select-Object -First 1
    $owned = @($windows | Where-Object { $_.ProcessId -eq $processId })
    Write-Output "process: $processId. name: $($process.Name). parent: $($process.ParentProcessId). top-level windows: $($owned.Count)"
    foreach ($window in $owned) {
      Write-Output ('window: ' + ($window | ConvertTo-Json -Compress -Depth 3))
    }
  }
}

function Wait-PickerStep([string]$step) {
  if ([DateTime]::UtcNow -ge $deadline) { throw "The shell folder dialog timed out during $step." }
  Start-Sleep -Milliseconds 100
}

function Describe-PickerControl($dialog, [string]$id) {
  try {
    $condition = [System.Windows.Automation.PropertyCondition]::new(
      [System.Windows.Automation.AutomationElement]::AutomationIdProperty, $id)
    $controls = $dialog.FindAll($descendants, $condition)
    if ($controls.Count -eq 0) { return "control id: $id. type: absent. class: absent. patterns: absent" }
    return (($controls | ForEach-Object {
      $patterns = @($_.GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName }) -join ', '
      if (-not $patterns) { $patterns = 'none' }
      "control id: $id. type: $($_.Current.ControlType.ProgrammaticName). class: $($_.Current.ClassName). patterns: $patterns. enabled: $($_.Current.IsEnabled). offscreen: $($_.Current.IsOffscreen)"
    }) -join ' | ')
  } catch {
    return "control id: $id. type: unavailable. class: unavailable. patterns: unavailable. UI Automation error: $($_.Exception.Message)"
  }
}

function Find-PickerControl($dialog, $condition, [string]$id) {
  # Reserve time for the control error and the existing window report.
  $controlDeadline = [DateTime]::UtcNow.AddSeconds(5)
  do {
    $controls = $dialog.FindAll($descendants, $condition)
    if ($controls.Count -gt 1) { throw "The dialog has more than one matching control. $(Describe-PickerControl $dialog $id)" }
    if ($controls.Count -eq 1 -and $controls[0].Current.IsEnabled -and -not $controls[0].Current.IsOffscreen) {
      return $controls[0]
    }
    if ([DateTime]::UtcNow -ge $controlDeadline -or [DateTime]::UtcNow -ge $deadline.AddSeconds(-2)) {
      throw "The dialog control is not ready. $(Describe-PickerControl $dialog $id)"
    }
    Start-Sleep -Milliseconds 100
  } while ($true)
}

function Get-PickerCloseFailure($dialog, $handle, $edit, [string]$path) {
  $address = 'unavailable'
  $field = 'unavailable'
  $navigated = $false
  try {
    $condition = [System.Windows.Automation.PropertyCondition]::new(
      [System.Windows.Automation.AutomationElement]::ClassNameProperty, 'ToolbarWindow32')
    $addresses = @($dialog.FindAll($descendants, $condition) | ForEach-Object { $_.Current.Name } | Where-Object { $_ -like 'Address:*' })
    if ($addresses.Count -gt 0) { $address = $addresses -join ' | ' }
  } catch {
    $address = "unavailable ($($_.Exception.Message))"
  }
  try {
    $field = [MunimentFolderPicker.Desktop]::FolderText([IntPtr]$handle, [IntPtr]$edit.Current.NativeWindowHandle)
    $navigated = $field.Length -gt 0 -and $field -cne $path
  } catch {
    $field = "unavailable ($($_.Exception.Message))"
  }
  if ($navigated) {
    return "The shell folder dialog navigated instead of closing. Displayed address: $address. Folder field: $field."
  }
  return "The shell folder dialog timed out during the window close. Displayed address: $address. Folder field: $field."
}

function Find-FolderDialog {
  $processIds = @(Get-Process -Name 'muniment-desktop' -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
  $matches = @([MunimentFolderPicker.Desktop]::Windows() | Where-Object {
    $_.ProcessId -in $processIds -and $_.Class -eq '#32770'
  })
  if ($matches.Count -gt 1) { throw 'Muniment has more than one shell dialog.' }
  if ($matches.Count -eq 1) {
    return [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$matches[0].Handle)
  }
}

$controlReport = $null
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
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty, 'Edit')
  )
  $confirmCondition = [System.Windows.Automation.AndCondition]::new(
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '1'),
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty, 'Button')
  )
  $controlReport = Describe-PickerControl $dialog '1152'
  $edit = Find-PickerControl $dialog $editCondition '1152'
  $controlReport = Describe-PickerControl $dialog '1152'
  [MunimentFolderPicker.Desktop]::SetFolder([IntPtr]$handle, [IntPtr]$edit.Current.NativeWindowHandle, $homePath)
  $controlReport = Describe-PickerControl $dialog '1'
  $confirm = Find-PickerControl $dialog $confirmCondition '1'
  $controlReport = Describe-PickerControl $dialog '1'
  [MunimentFolderPicker.Desktop]::ConfirmFolder([IntPtr]$handle, [IntPtr]$confirm.Current.NativeWindowHandle)
  while ($null -ne ([MunimentFolderPicker.Desktop]::Windows() | Where-Object {
    $_.Handle -eq $handle -and $_.Visible
  })) {
    if ([DateTime]::UtcNow -ge $deadline) {
      throw (Get-PickerCloseFailure $dialog $handle $edit $homePath)
    }
    Start-Sleep -Milliseconds 100
  }
} catch {
  $failure = $_
  try {
    Write-PickerWindows
  } catch {
    Write-Output "window report error: $($_.Exception.Message)"
  }
  if ($controlReport) { throw "$($failure.Exception.Message) $controlReport" }
  throw $failure
}
