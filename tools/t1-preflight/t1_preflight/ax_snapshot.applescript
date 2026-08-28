on run arguments
    if (count of arguments) is not 1 then error "stage=ax.arguments cause=expected-pid"
    set targetPid to (item 1 of arguments) as integer
    tell application "System Events"
        set matches to every application process whose unix id is targetPid
        if (count of matches) is not 1 then error "stage=ax.process expected=1 actual=" & (count of matches)
        set targetProcess to item 1 of matches
        tell targetProcess
            if (count of windows) is 0 then error "stage=ax.window cause=missing"
            set outputLines to {"pid" & tab & targetPid, "process" & tab & (name as text), "windows" & tab & ((count of windows) as text)}
            set allElements to entire contents of window 1
            repeat with currentElement in allElements
                try
                    set elementRole to role of currentElement as text
                    set elementDescription to description of currentElement
                    if elementDescription is missing value then set elementDescription to ""
                    set elementTitle to title of currentElement
                    if elementTitle is missing value then set elementTitle to ""
                    set end of outputLines to elementRole & tab & (elementDescription as text) & tab & (elementTitle as text)
                on error errorMessage
                    set end of outputLines to "AXError" & tab & errorMessage
                end try
            end repeat
            set AppleScript's text item delimiters to linefeed
            return outputLines as text
        end tell
    end tell
end run
