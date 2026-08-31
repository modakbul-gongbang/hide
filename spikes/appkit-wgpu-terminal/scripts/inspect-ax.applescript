on run arguments
    set targetPid to (item 1 of arguments) as integer
    tell application "System Events"
        set targetProcess to first application process whose unix id is targetPid
        set frontmost of targetProcess to true
        tell targetProcess
            set outputLines to {"process=" & (name as text), "window_count=" & ((count of windows) as text)}
            if (count of windows) is 0 then error "stage=ax.query cause=no-window"
            set windowRole to role of window 1
            set end of outputLines to "window_role=" & windowRole
            set allElements to entire contents of window 1
            repeat with currentElement in allElements
                try
                    set elementRole to role of currentElement
                    set elementDescription to description of currentElement
                    if elementDescription is missing value then set elementDescription to ""
                    set elementTitle to title of currentElement
                    if elementTitle is missing value then set elementTitle to ""
                    set end of outputLines to "role=" & elementRole & " description=" & elementDescription & " title=" & elementTitle
                end try
            end repeat
            set AppleScript's text item delimiters to linefeed
            return outputLines as text
        end tell
    end tell
end run
