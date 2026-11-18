# Run unchanged on the native Shell and a reference POSIX shell.
original_path=$PATH
set -- first 'two words' third
cat > sourced.sh <<'SOURCE'
loaded='value with spaces'
sourced_function() { echo "function:[$loaded]"; }
echo "dot-args:$0:$#:$1:[$2]"
shift
SOURCE
. ./sourced.sh
echo "shared-args:$#:$1:$2"
sourced_function
echo "shared-variable:[$loaded]"

false
. ./empty.sh
echo "empty:$?"
false
. ./comment.sh
echo "comment:$?"
. ./noend.sh
echo "no-newline-status:$?"

cat > pick.sh <<'SOURCE'
picked=yes
SOURCE
PATH=SUB:/unrepresentable_directory:. . pick.sh
/bin/echo "path:$picked:$PATH"
PATH=$original_path
cat > -src <<'SOURCE'
/bin/echo dash-name
SOURCE
. -- ./-src
PATH=. . -- -src
PATH=$original_path

cat > inner.sh <<'SOURCE'
echo inner
return 13
echo BAD-inner
SOURCE
cat > outer.sh <<'SOURCE'
. ./inner.sh
echo "outer-resumed:$?"
return 14
echo BAD-outer
SOURCE
. ./outer.sh
echo "caller-resumed:$?"
function_dot() {
    . ./inner.sh
    echo "function-resumed:$?"
    return 19
    echo BAD-function
}
function_dot
echo "function-status:$?"
cat > funcdot.sh <<'SOURCE'
inside() { return 21; }
inside
echo "inside-function:$?"
return 22
SOURCE
. ./funcdot.sh
echo "dot-function-status:$?"

cat > retlast.sh <<'SOURCE'
# The first command must see the caller's status, even after a comment.
return
SOURCE
false
. ./retlast.sh
echo "return-last:$?"
cat > retloop.sh <<'SOURCE'
for v in first second; do
    echo "loop:$v"
    return 23
done
echo BAD-loop
SOURCE
. ./retloop.sh
echo "loop-return:$?"

cat > stdin.sh <<'SOURCE'
read -r input
echo "stdin:[$input]"
sh -c 'read -r hidden <&10' > fdout.tmp 2> fderr.tmp
case $? in 0) echo BAD-descriptor-10;; *) echo descriptor-10-closed;; esac
sh -c 'read -r hidden <&11' > fdout.tmp 2> fderr.tmp
case $? in 0) echo BAD-descriptor-11;; *) echo descriptor-11-closed;; esac
SOURCE
. ./stdin.sh <<'INPUT'
ordinary stdin
INPUT

cat > cwd.sh <<'SOURCE'
cd SUB
SOURCE
. ./cwd.sh
case $PWD in */SUB) echo changed-directory;; *) echo "BAD-cwd:$PWD";; esac
cd ..

cat > multi.sh <<'SOURCE'
if true; then
    cat <<'TEXT'
multiline:literal $data
TEXT
fi
SOURCE
. ./multi.sh > dotfile.out
cat dotfile.out

eval
echo "eval-none:$?"
false
eval '' ''
echo "eval-null:$?"
eval ' # comment'
echo "eval-comment:$?"
eval 'evaluated="two words"' '; echo' '"eval-value:[$evaluated]"'
echo "eval-persists:[$evaluated]"
name=evaluated
eval 'echo "indirect:[$'"$name"']"'
eval 'echo eval-pipeline | cat > evalfile.out'
cat evalfile.out
eval 'f_eval() { echo eval-function; }; f_eval'
f_eval
false
eval 'echo "eval-previous:$?"'
eval 'false'
echo "eval-status:$?"
eval_return() {
    eval 'return 29'
    echo BAD-eval-return
}
eval_return
echo "eval-function-return:$?"
negated_return() { ! return 30; echo BAD-negated-return; }
negated_return
echo "negated-return:$?"
cat > evalret.sh <<'SOURCE'
eval 'return 31'
echo BAD-eval-dot-return
SOURCE
. ./evalret.sh
echo "eval-dot-return:$?"
assigned=before
assigned=prefix eval 'echo "eval-prefix:$assigned"; assigned=after'
echo "eval-prefix-persists:$assigned"

reject() {
    if sh -c "$2" > srcerr.out 2> srcerr.err; then
        echo "BAD-success:$1"
    elif read -r line < srcerr.out; then
        echo "BAD-continued:$1:$line"
    elif read -r line < srcerr.err; then
        echo "reject:$1"
    else
        echo "BAD-no-diagnostic:$1"
    fi
}
reject missing '. ./missing.sh; echo BAD'
reject no-operand '.; echo BAD'
reject empty-name '. ""; echo BAD'
reject no-current-search 'PATH=/bin; . pick.sh; echo BAD'
reject directory '. ./SUB; echo BAD'
reject bad-option '. -Q; echo BAD'
reject eval-syntax 'eval "if ;"; echo BAD'
reject eval-incomplete 'eval "if true; then"; echo BAD'
reject dot-syntax '. ./broken.sh; echo BAD'
reject readonly-eval 'readonly v=old; eval "v=new"; echo BAD'

sh -c '. ./exitdot.sh; echo BAD'
echo "dot-exit:$?"
sh -c 'eval "exit 41"; echo BAD'
echo "eval-exit:$?"
i=0
while :; do
    . ./empty.sh
    i=$((i + 1))
    case $i in 256) break;; esac
done
echo "repeated-dot:$i"
exit 53
