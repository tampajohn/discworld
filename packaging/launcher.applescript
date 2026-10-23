-- SPDX-License-Identifier: GPL-3.0-or-later
on run
    try
        set gameISO to choose file with prompt "Choose your English DOS CD v2 Discworld ISO"
        set musicFont to choose file with prompt "Choose a SoundFont (.sf2) for music"
        set supportRoot to POSIX path of (path to application support from user domain) & "Discworld Rust/"
        set runtimePath to POSIX path of (path to me) & "Contents/Resources/runtime/discworld-native"
        do shell script "/bin/mkdir -p " & quoted form of supportRoot
        set logPath to supportRoot & "last-session.log"
        set savePath to supportRoot & "quicksave.dws"
        -- Every filesystem argument is separately shell-quoted, including paths
        -- selected by the user. No game files are written into the signed bundle.
        set launchCommand to quoted form of runtimePath & " " & quoted form of (POSIX path of gameISO) & " " & quoted form of (POSIX path of musicFont) & " --save-file " & quoted form of savePath & " > " & quoted form of logPath & " 2>&1"
        try
            do shell script launchCommand
        on error messageText number errorNumber
            display alert "Discworld could not finish this session" message ("See " & logPath & " for details. Exit code: " & errorNumber) as critical
        end try
    on error messageText number errorNumber
        if errorNumber is not -128 then
            display alert "Discworld could not start" message messageText as critical
        end if
    end try
end run
