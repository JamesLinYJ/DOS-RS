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
if ( true ); then echo conditional-subshell; fi
exec 3> redir.tmp
echo saved >&3
{ echo scoped; } 4>&1 1>&3 4>&-
exec 3>&-
exec 3< redir.tmp
cat <&3
exec 3<&-
echo restored
for item in abc foobar '*' z; do
    case "$item" in
        (a?c|123) echo first ;;
        'foo*') echo wrong-literal ;;
        foo*) echo wildcard ;;
        "*") echo literal-star ;;
        [x-z]) echo range ;;
    esac
done
case miss in hit) echo wrong; esac
echo "no-match:$?"
case x in x) ;; esac
echo "empty:$?"
hold=outer
capture=$(hold=inner; echo "$hold"; echo; echo; exit 27)
echo "capture:$?:$capture:$hold"
echo "nested:$(echo "$(echo nested)")"
echo "case:$(case x in x) echo yes ;; esac)"
legacy=`echo older`
echo "legacy:$legacy"
origin=$$
case "$(echo $$)" in "$origin") echo same-dollar ;; *) echo wrong-dollar ;; esac
mark() { echo mark >> mark.tmp; echo echo; }
: > mark.tmp
$(mark) once
for rows in $(wc -l < mark.tmp); do echo "marks:$rows"; done
marker=$(echo first; echo second)
for part in $marker; do echo "field:$part"; done
echo "quoted:$marker"
echo via > "$(echo path.tmp)"
cat path.tmp
bulk=x
for k in a b c d e f g h i j k l m; do bulk="$bulk$bulk"; done
result=$(echo "$bulk")
for bytes in $(echo "$result" | wc -c); do echo "bytes:$bytes"; done
echo "radices:$((0x11 + 012 * (3 + 1)))"
neg=-012
echo "signed:$((neg)):$((+7 % 3)):$((~-1))"
echo "assignment:$((n = m = 7)):$n:$m"
echo "compound:$((n *= (m -= 2)))"
echo "after:$n:$m"
echo "logical:$((0 && (n=100))):$((1 || (1/0))):$n"
echo "selection:$((0 ? 1/0 : m == 5 ? 21 : 0))"
echo "bits:$(((((2 << 4) | 3) ^ 2) & 63)):$((32 >> 2))"
echo "expanded:$(( ${m} + $(echo 3) * $((1+1)) ))"
echo "wide:$((4294967296 + 1))"
echo "subshell:$( (echo grouped) )"
if sh -c 'echo "$((1/0))"; echo wrong-zero' 2> arith.err; then echo wrong-status; else echo arithmetic-error; fi
PATH=.:$PATH SCRIPT_VALUE=path plain "two words" ""
echo "path-script:$?"
SCRIPT_VALUE=direct ./plain direct
echo "direct-script:$?"
SCRIPT_VALUE=literal sh -- plain opt
echo "shell-options:$?"
exit 13
