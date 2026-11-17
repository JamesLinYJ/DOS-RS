# Optional dot arguments follow the KornShell extension described by Issue 8.
set -- caller 'caller words'
cat > dotargs.sh <<'SOURCE'
echo "local:$0:$#:$1:[$2]"
shift
set -- replacement
. ./nested.sh nested 'nested words'
echo "after-nested:$#:$1"
return 17
echo BAD-after-return
SOURCE
cat > nested.sh <<'SOURCE'
echo "nested:$0:$#:$1:[$2]"
set -- nested-change
return 13
SOURCE
. ./dotargs.sh supplied 'supplied words'
echo "restored:$?:$0:$#:$1:[$2]"
exit 59
