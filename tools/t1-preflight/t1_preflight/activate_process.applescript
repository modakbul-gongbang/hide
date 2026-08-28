on run arguments
    if (count of arguments) is not 1 then error "stage=input.arguments cause=expected-pid"
    set targetPid to (item 1 of arguments) as integer
    tell application "System Events"
        set matches to every application process whose unix id is targetPid
        if (count of matches) is not 1 then error "stage=input.process expected=1 actual=" & (count of matches)
        set frontmost of item 1 of matches to true
    end tell
end run
