exec 3< read.dat
read first rest <&3
echo "space:$?:[$first]:[$rest]"
IFS=' :' read first middle rest <&3
echo "escaped:$?:[$first]:[$middle]:[$rest]"
read joined tail <&3
echo "continued:$?:[$joined]:[$tail]"
IFS= read -r raw after <&3
echo "raw:$?:[$raw]:[$after]"
IFS=: read -d '|' first rest <&3
echo "remainder:$?:[$first]:[$rest]"
IFS= read -rd '|' item <&3
echo "next:$?:[$item]"
read -r -d '' item <&3
echo "nul:$?:[$item]"
read first rest <&3
echo "partial:$?:[$first]:[$rest]"
read -- first rest <&3
echo "eof:$?:[$first]:[$rest]"
exec 3<&-
echo 'piped value' | { read left right; echo "pipe:[$left]:[$right]"; }
set -- after positional
shift
echo "remain:$#:$1"
exit 31
