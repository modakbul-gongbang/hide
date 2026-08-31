on run arguments
    set targetPid to (item 1 of arguments) as integer
    tell application "System Events"
        set targetProcess to first application process whose unix id is targetPid
        set frontmost of targetProcess to true
        delay 0.2
        keystroke return using {command down, shift down}
    end tell
end run
