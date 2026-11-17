# Issue 8 section 2.6.5 consumes IFS whitespace adjacent to a nonwhite delimiter.
IFS=' :'
a='x '
b=': y'
set -- $a$b
echo "$#:[$1]:[$2]"
set -- $a":"$b
echo "$#:[$1]:[$2]:[$3]"
exit 23
