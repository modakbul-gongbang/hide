on run arguments
    if (count of arguments) is not 3 then error "stage=resize.arguments cause=expected-pid-width-height"
    set targetPid to (item 1 of arguments) as integer
    set targetWidth to (item 2 of arguments) as integer
    set targetHeight to (item 3 of arguments) as integer
    tell application "System Events"
        set matches to every application process whose unix id is targetPid
        if (count of matches) is not 1 then error "stage=resize.process expected=1 actual=" & (count of matches)
        tell item 1 of matches
            if (count of windows) is not 1 then error "stage=resize.window expected=1 actual=" & (count of windows)
            set size of window 1 to {targetWidth, targetHeight}
        end tell
    end tell
end run
