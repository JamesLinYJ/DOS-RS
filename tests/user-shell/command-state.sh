value='two words'
command export declared=$value
command command export nested_declared=$value
HOME='/home/space name'
command export expanded=~/path
echo "declare:$declared:$nested_declared:$expanded"
sh -c 'echo "exported:$declared:$nested_declared"'
token=outside
token=inside command :
echo "prefix:$token"
token=inside command eval 'saved=$token'
echo "eval:$token:$saved"
token=inside command export other=value
echo "export-prefix:$token:$other"
token=inside command . ./cmdsrc.sh
echo "source-prefix:$token:$sourced"
f() { command return 23; echo BAD-return; }
f
echo "function-return:$?"
command(){ /bin/echo "command-function:$*"; }
command echo shadowed
unset -f command
command -- echo restored
command
echo "no-operand:$?"
command --
echo "delimiter-only:$?"
command echo piped | cat > cmdpipe.out
cat cmdpipe.out
command echo background > cmdbg.out &
wait
cat cmdbg.out
sh -c 'command exec > cmdexec.out; echo permanent'
cat cmdexec.out
sh -c 'command exec echo replaced; echo BAD-exec'
sh -c 'command exit 29; echo BAD-exit'
echo "exit:$?"
sh -c 'command eval "exit 31"; echo BAD-eval-exit'
echo "eval-exit:$?"
sh -c 'command . ./exitdot.sh; echo BAD-dot-exit'
echo "dot-exit:$?"
path_before=$PATH
PATH=./CMDDIR:/bin command -v cat
case "$PATH" in "$path_before") echo query-prefix-restored;; *) echo BAD-query-prefix;; esac
PATH=./CMDDIR:/bin command cat cmddata.txt
command -v ./plain > cmdv.out
read -r path < cmdv.out
case "$path" in /*/plain) echo absolute-path;; *) echo BAD-absolute;; esac
PATH= command -v plain > cmdv.out
read -r path < cmdv.out
case "$path" in /*plain) echo empty-path;; *) echo BAD-empty;; esac
if PATH=./SUB command -v plain > cmdv.out; then echo BAD-cwd-fallback; else echo no-cwd-fallback; fi
if command -v SUB > cmdv.out; then echo BAD-directory-query; else echo directory-rejected; fi
command ./SUB 2> cmd.err
echo "directory-exec:$?"
command absent-command 2> cmd.err
echo "missing-exec:$?"
command '' 2> cmd.err
echo "empty-exec:$?"
command if 2> cmd.err
echo "keyword-exec:$?"
command -- -v 2> cmd.err
echo "option-as-name:$?"
long=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
PATH=./$long:/bin command -v cat
PATH=./$long:/bin command cat cmddata.txt
exit 67
