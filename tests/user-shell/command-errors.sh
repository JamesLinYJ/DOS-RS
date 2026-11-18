for code in 'command export 1bad' 'command unset 1bad' 'command . ./absent' 'command exec ./absent' 'command exec < ./absent' 'command eval : < ./absent'; do
    sh -c "$code; echo continued" > cmderr.out 2> cmd.err
    if read -r line < cmderr.out; then echo "$line"; else echo BAD-aborted; fi
done
for code in 'command -q echo BAD' 'command -v' 'command -V' 'command -vV echo'; do
    sh -c "$code" > cmderr.out 2> cmd.err
    status=$?
    if read -r line < cmderr.out; then echo BAD-invalid-output; else echo invalid-output-empty; fi
    echo "invalid-status:$status"
done
# Errors stop the evaluated input, but command suppresses abort of its caller.
# Explicit exit is checked independently in command-state.sh.
for code in 'command eval "if ;"' 'command eval "export 1bad; echo BAD-inner"' 'command . ./cmdbad.sh'; do
    sh -c "$code; echo nested-error-returned" > cmderr.out 2> cmd.err
    cat cmderr.out
done
sh -c 'command export x=original; command readonly x; command export x=changed; echo "$x"' 2> cmd.err
exit 71
