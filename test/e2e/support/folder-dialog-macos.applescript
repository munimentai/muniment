on waitForStep(deadline, stepName)
  if (current date) >= deadline then error "The NSOpenPanel timed out during " & stepName & "."
  delay 0.1
end waitForStep

on findPanel(appProcess)
  tell application "System Events"
    set matches to {}
    repeat with appWindow in windows of appProcess
      set candidates to {appWindow} & (sheets of appWindow)
      repeat with candidate in candidates
        repeat with buttonName in {"Open", "Choose"}
          if exists button (contents of buttonName) of candidate then
            set end of matches to {contents of candidate, button (contents of buttonName) of candidate}
            exit repeat
          end if
        end repeat
      end repeat
    end repeat
    if (count matches) > 1 then error "Muniment has more than one Open or Choose panel."
    if (count matches) is 1 then return item 1 of matches
    return missing value
  end tell
end findPanel

on run argv
  set homePath to item 1 of argv
  set deadline to (current date) + (item 2 of argv as integer)
  tell application "System Events"
    try
      set appProcesses to application processes whose bundle identifier is "ai.muniment.desktop"
      repeat while (count appProcesses) is 0
        my waitForStep(deadline, "the process search")
        set appProcesses to application processes whose bundle identifier is "ai.muniment.desktop"
      end repeat
      if (count appProcesses) is not 1 then error "The Muniment process is ambiguous."
      set appProcess to item 1 of appProcesses
      set panelMatch to my findPanel(appProcess)
      repeat while panelMatch is missing value
        my waitForStep(deadline, "the window search")
        set panelMatch to my findPanel(appProcess)
      end repeat
      set pickerPanel to item 1 of panelMatch
      set confirmButton to item 2 of panelMatch
      try
        log "window: " & (name of pickerPanel) & ". process: " & (name of appProcess)
      end try
      set frontmost of appProcess to true
      if not (frontmost of appProcess) then error "Muniment does not have keyboard focus."
      keystroke "g" using {command down, shift down}
      set pathField to missing value
      repeat while pathField is missing value
        my waitForStep(deadline, "the Go to Folder field search")
        repeat with goToSheet in sheets of pickerPanel
          repeat with control in entire contents of goToSheet
            if role of control is "AXTextField" then
              set pathField to contents of control
              set pathSheet to contents of goToSheet
              exit repeat
            end if
          end repeat
          if pathField is not missing value then exit repeat
        end repeat
      end repeat
      set value of pathField to homePath
      if (value of pathField) is not homePath then error "The Go to Folder field did not keep the Home path."
      if not (frontmost of appProcess) then error "Muniment lost keyboard focus."
      key code 36
      repeat while exists pathSheet
        my waitForStep(deadline, "the Go to Folder sheet close")
      end repeat
      repeat while not (enabled of confirmButton)
        my waitForStep(deadline, "the Open or Choose button")
      end repeat
      click confirmButton
      repeat while (my findPanel(appProcess)) is not missing value
        my waitForStep(deadline, "the window close")
      end repeat
    on error errorMessage number errorNumber
      try
        repeat with appProcess in (application processes whose bundle identifier is "ai.muniment.desktop")
          log "visible windows: " & (name of every window of appProcess)
        end repeat
      end try
      error errorMessage number errorNumber
    end try
  end tell
end run
