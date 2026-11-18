# Redirection failure is fatal only for a directly invoked special built-in.
for code in '. ./empty.sh < ./absent' 'eval : < ./absent'; do
    if sh -c "$code; echo BAD" > redir.out 2> redir.err; then
        echo BAD-redirection-success
    elif read -r line < redir.out; then
        echo "BAD-redirection-continued:$line"
    elif read -r line < redir.err; then
        echo special-redirection-rejected
    else
        echo BAD-no-redirection-diagnostic
    fi
done
sh -c 'true < ./absent; echo regular-redirection-continued' 2> redir.err
exit 61
