on run arguments
    if (count of arguments) is not 3 then error "stage=input.arguments cause=expected-pid-x-y"
    set targetPid to (item 1 of arguments) as integer
    set targetX to (item 2 of arguments) as integer
    set targetY to (item 3 of arguments) as integer
    tell application "System Events"
        set matches to every application process whose unix id is targetPid
        if (count of matches) is not 1 then error "stage=input.process expected=1 actual=" & (count of matches)
        tell item 1 of matches
            if (count of windows) is 0 then error "stage=input.window cause=missing"
            set position of window 1 to {targetX, targetY}
        end tell
    end tell
end run
