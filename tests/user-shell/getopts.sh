echo "initial:$OPTIND"
unset OPTARG
set -- -ac -b 'two words' -dATTACHED -- -a operand
while getopts ':ab:cd:' option; do
    echo "scan:$option:[${OPTARG-unset}]"
done
echo "end:$option:$OPTIND:${OPTARG+x}"
shift "$((OPTIND - 1))"
echo "operands:$#:$1:$2"

OPTIND=1
getopts ':b:' option -b ''
echo "empty:$?:$option:$OPTIND:[${OPTARG-unset}]"
OPTIND=1
getopts ':b:' option -b --
echo "dasharg:$?:$option:$OPTIND:[$OPTARG]"
OPTIND=1
getopts ':b:' option -b -9
echo "negative:$?:$option:$OPTIND:[$OPTARG]"
OPTIND=1
getopts ':b:' option -b '中文한글é'
echo "utf8:$?:$option:$OPTIND:[$OPTARG]"
sh -c 'echo "argument-export:${OPTARG+x}"'

OPTIND=1
while getopts ':ab:' option -za -b; do
    echo "silent:$option:[${OPTARG-unset}]"
done
echo "silent-end:$option:$OPTIND:${OPTARG+x}"
OPTIND=1
getopts a option -x 2> opts.err
echo "unknown:$?:$option:${OPTARG+x}"
read -r message < opts.err
case "$message" in *getopts.sh*x*) echo unknown-diagnostic;; *) echo BAD-unknown-diagnostic;; esac
OPTIND=1
getopts b: option -b 2> opts.err
echo "missing:$?:$option:${OPTARG+x}"
read -r message < opts.err
case "$message" in *getopts.sh*b*) echo missing-diagnostic;; *) echo BAD-missing-diagnostic;; esac

set -- -ab
OPTIND=1
getopts ab option
echo "cluster:$option"
OPTIND=1
getopts ab option
echo "restart:$option"
export OPTIND=1
getopts ab option
echo "export-restart:$option"
read OPTIND <<INDEX
1
INDEX
getopts ab option
echo "read-restart:$option"
: "$((OPTIND = 1))"
getopts ab option
echo "arithmetic-restart:$option"
(getopts ab option; echo "subshell:$option")
getopts ab option
echo "parent:$option"

OPTIND=1
getopts a option -a
echo "explicit:$?:$option:$OPTIND:$#:$1"
scan() {
    OPTIND=1
    getopts 'b:' option
    echo "function:$option:[$OPTARG]:$OPTIND"
}
scan -b function-arg
echo "after-function:$#:$1"

OPTIND=41
sh -c 'echo "new-shell:$OPTIND"'
set --
OPTIND=1
getopts a option
echo "no-args:$?:$option:$OPTIND:${OPTARG+x}"
OPTIND=1
getopts a option - -a
echo "lone-dash:$?:$option:$OPTIND"
OPTIND=1
getopts a option operand -a
echo "first-operand:$?:$option:$OPTIND"
OPTIND=1
getopts a invalid-name -a 2> opts.err
case $? in 0|1) echo BAD-invalid-name;; *) echo invalid-name;; esac
getopts 2> opts.err
case $? in 0|1) echo BAD-usage;; *) echo invalid-usage;; esac
exit 43
