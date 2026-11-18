unset v r empty fixed
export v
echo "export-unset:${v+x}"
sh -c 'echo "child-unset:${v+x}"'
v=
echo "export-empty:${v+x}:[$v]"
sh -c 'echo "child-empty:${v+x}:[$v]"'
v='two words'
sh -c 'echo "child-value:[$v]"'
unset -v v
v=private
sh -c 'echo "child-unexported:${v+x}"'

readonly r
readonly empty=
words='two words * ? 中文'
readonly fixed=$words
readonly fixed
echo "readonly-unset:${r+x}"
echo "readonly-empty:${empty+x}:[$empty]"
echo "readonly-value:[$fixed]"
export fixed r
sh -c 'echo "child-readonly:[$fixed]:${r+x}"'
(
    readonly scoped=inner
    echo "subshell:[$scoped]"
)
echo "outside:${scoped+x}"

readonly quoted="a'b * ? 中文"
readonly metachar='$(echo BAD) $HOME ; \ end'
readonly -p > roall.tmp
while IFS= read -r declaration; do
    case "$declaration" in
        'readonly r'|'readonly empty='*|'readonly fixed='*|'readonly quoted='*|'readonly metachar='*)
            echo "$declaration";;
    esac
done < roall.tmp > rostate.sh
cat >> rostate.sh <<'CHECK'
echo "restored-readonly:${r+x}:${empty+x}:[$fixed]:[$quoted]:[$metachar]"
r=changed
echo BAD-restored-readonly
CHECK
sh rostate.sh 2> varerr.err
case $? in 0) echo BAD-restored-readonly-status;; *) echo readonly-restored-protected;; esac

export unassigned
export quoted
export -p > exall.tmp
while IFS= read -r declaration; do
    case "$declaration" in
        'export unassigned'|'export quoted='*) echo "$declaration";;
    esac
done < exall.tmp > exstate.sh
cat >> exstate.sh <<'CHECK'
sh -c 'echo "restored-export:${unassigned+x}:[$quoted]"'
CHECK
sh -c 'unset quoted unassigned; exec sh exstate.sh'

unset -- absent
echo "unset-absent:$?"
fixture_function_uniq=variable
fixture_function_uniq() { echo function-present; }
unset -v fixture_function_uniq
fixture_function_uniq
echo "variable-removed:${fixture_function_uniq+x}"
fixture_function_uniq=kept
unset -f fixture_function_uniq
fixture_function_uniq 2> varerr.err
echo "function-removed:$?:[$fixture_function_uniq]"
PATH=/unrepresentable_directory:/bin echo path-search-continues

read fixed 2> varerr.err <<VALUE
replacement
VALUE
case $? in 0) echo BAD-read;; *) echo "read-protected:[$fixed]";; esac
getopts a fixed -a 2> varerr.err
case $? in 0|1) echo BAD-getopts;; *) echo "getopts-protected:[$fixed]";; esac

reject() {
    if sh -c "$2" > varerr.out 2> varerr.err; then
        echo "BAD-success:$1"
    elif read -r line < varerr.out; then
        echo "BAD-continued:$1:$line"
    elif read -r line < varerr.err; then
        echo "reject:$1"
    else
        echo "BAD-no-diagnostic:$1"
    fi
}
reject assignment 'readonly v=old; v=new; echo BAD'
reject external-prefix 'readonly v=old; v=new echo BAD; echo BAD'
reject builtin-prefix 'readonly v=old; v=new true; echo BAD'
reject function-prefix 'f() { echo BAD; }; readonly v=old; v=new f; echo BAD'
reject export 'readonly v=old; export v=new; echo BAD'
reject readonly 'readonly v=old; readonly v=new; echo BAD'
reject unset 'readonly v=old; unset v; echo BAD'
reject loop 'readonly v=old; for v in new; do echo BAD; done; echo BAD'
reject arithmetic 'readonly v=1; : "$((v = 2))"; echo BAD'
reject default 'readonly v; : "${v:=new}"; echo BAD'
reject negated '! { readonly v=old; v=new; }; echo BAD'
reject export-name 'export invalid-name; echo BAD'
reject readonly-name 'readonly invalid-name; echo BAD'
reject unset-name 'unset invalid-name; echo BAD'
reject export-option 'export -Q; echo BAD'
reject readonly-option 'readonly -Q; echo BAD'
reject unset-option 'unset -Q; echo BAD'
reject export-output 'export -p >&-; echo BAD'
reject readonly-output 'readonly v=value; readonly -p >&-; echo BAD'
exit 47
