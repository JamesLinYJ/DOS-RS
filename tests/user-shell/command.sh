# command must choose execution before deciding whether to fork.
echo() { /bin/echo "function:$*"; }
echo shadow
command echo external
command command echo nested
unset -f echo
cd() { command cd "$@"; command pwd; }
before=$(command pwd)
cd SUB > cmdcwd.out
case "$PWD" in */SUB) echo changed-directory;; *) echo BAD-directory;; esac
command cd "$before"
unset -f cd

set -- one two
command shift
echo "shift:$#:$1"
command set -- alpha 'two words'
echo "set:$#:$1:$2"
function_only() { echo BAD-function-only; }
command function_only 2> cmd.err
echo "function-only:$?"
command -v function_only
command -V function_only > cmdv.out
read -r description < cmdv.out
case "$description" in *function*) echo function-description;; *) echo BAD-description;; esac
command -v export
command -v cd
command -v if
command -V export > cmdv.out
read -r description < cmdv.out
case "$description" in *special*built*) echo special-description;; *) echo BAD-special;; esac
command -V cd > cmdv.out
read -r description < cmdv.out
case "$description" in *built*) echo builtin-description;; *) echo BAD-builtin;; esac
command -V if > cmdv.out
read -r description < cmdv.out
case "$description" in *reserved*|*keyword*) echo reserved-description;; *) echo BAD-reserved;; esac
if command -v absent-command > cmdv.out; then echo BAD-missing-status; else echo query-missing; fi
if read -r description < cmdv.out; then echo BAD-missing-output; else echo missing-query-silent; fi

path_before=$PATH
PATH=./SUB
command -p echo default-path
command -p -v cat
command -pv cat
command -p -V cat > cmdv.out
read -r description < cmdv.out
case "$description" in */bin/cat*) command -p echo external-description;; *) command -p echo BAD-external;; esac
command -p echo "preserved-path:$PATH"
readonly PATH
command -p echo readonly-path
exit 65
