show() {
    line=$#
    for part; do line="$line|<$part>"; done
    echo "$line"
}
unset missing
empty=''
full=present
show "${missing-fall}" "${missing:-fall}" "${empty-fall}" "${empty:-fall}" "${full:+ok}"
show "${missing+bad}" "${empty+set}" "${empty:+bad}" "${full+ok}"
show "${missing:-'${full}'}" ${missing:-'a b'} "${missing:-a\}b}"
show "${missing:-${other:-$(echo nest)}}" "${full:-$(echo BAD > side.err)}"
show "${full:=unused}" "${saved:=assigned value}" "$saved"
saved=''
show "${saved=keep}" "${saved:=new}" "$saved"
set -- 1 2 3 4 5 6 7 8 9 ten eleven
show "${10}-${11}" "${#full}" "${#10}" "${#}"
shift 9
show pre"$@"post
set -- "" "a b" ""
show pre"$@"post "${missing:-"$@"}"
set --
show "$@" "" "$@"
path=dir/long/file.txt
show "${path#*/}" "${path##*/}" "${path%/*}" "${path%%/*}"
pat='*/'
show "${path##$pat}" "${path##"$pat"}" "${path#dir/?}" "${path#*}"
star='*here'
show "${star#'*'}" "${star##\*}" "${star%%*}"
v=' :aa :: bb: :'
IFS=' :'
show $v
show "a"$v'z' aa:bb "${v}"
a='x '
b=': y'
show $a"middle"$b
unset IFS
set -- left "" "two words"
show $@ "$*" pre"$@"post
IFS=:
show "$*" $* "$@"
IFS=''
show "$*" $@ $* "$@"
unset IFS
: > G1.DAT
: > G2.DAT
show G[12].DAT ./G?.DAT 'G*'.DAT G\*.DAT N*.QQ
wild='G*.DAT'
show $wild "$wild" ${missing:-G?.DAT} ${missing:-'G?.DAT'}
set -f
show G?.DAT $wild
case "$-" in *f*) echo glob-disabled ;; esac
set +f
case "$-" in *f*) echo wrong-flags ;; *) echo glob-restored ;; esac
if sh -uc 'echo "$absent"; exit 0' 2> err.tmp; then echo wrong-nounset; else echo nounset-failed; fi
if sh -c 'echo "${absent:?need input}"; exit 0' 2> err.tmp; then echo wrong-required; else echo required-failed; fi
if sh -uc 'echo "${absent:-fallback}"' > opts.tmp; then cat opts.tmp; fi
HOME='two words/G*.DAT'
show ~ ~/dir '~' ~"/dir" "~"
paths=~:~/nested:~NO_SUCH_DOS_ACCOUNT
show "$paths"
export example=~/file more=~:~/"quoted"
show "$example" "$more"
show arg=~:~/here "${missing:-~}"
cd SUB
show *.TX ../G[12].DAT
cd ..
show marker-end
echo ok
exit 17
