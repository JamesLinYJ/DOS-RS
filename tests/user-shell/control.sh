label=parent
if false; then
    echo wrong-if
elif ! false && true; then
    echo if-ok
else
    echo wrong-else
fi
if false; then echo wrong; fi
echo "empty-if:$?"
for item; do
    echo "<$item>"
done
for item in ""; do echo "empty=[$item]"; done
for nothing in; do echo wrong-empty; done
while true; do
    for item in first second; do
        echo "break:$item"
        break 2
    done
    echo wrong-loop
done
ready=false
until $ready; do
    echo until
    ready=true
    continue
    echo wrong-continue
done
for parent in a b; do
    for child in c d; do echo "$parent:$child"; continue 2; done
    echo wrong-nested
done
( label=child; echo "$label" )
echo "$label"
{ label=shared; echo "brace:$label"; } | cat
echo "after-pipe:$label"
{ label=shared; }
echo "after-brace:$label"
echo 'if' 'then' 'fi'
! true
echo "negated:$?"
false ||
  echo or-ok
echo pipe |
  cat
f() {
    echo "f:$#:$1:$2"
    for ignored in 1 2; do return 17; done
    echo wrong-return
}
f one "two words"
echo "returned:$?:$#:$1"
g() { echo "[$item]"; }
h() { g; }
item=original
item=temporary h
# Issue 8 leaves persistence of function-prefix assignments unspecified.
item=original
h
k() { for item in "$@"; do echo wrong-empty-at; done; }
k
( f nested; exit 0 )
echo "parent:$?"
isolated() ( item=hidden; echo "$item"; return 4 )
isolated
echo "isolated:$?:$item"
exit 13
