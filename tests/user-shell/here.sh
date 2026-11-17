name='two words'
cat <<MSG
plain: $name / "$name" / 'quotes' / tilde~ / * / $((2 + 3))
escapes: \$name / \`literal\` / \\ / \"quoted\"
live: $(echo inner) / ${missing:-fallback}
MSG
cat <<'END QUOTED'
$name ` "$(echo literal)" \\
END QUOTED
cat <<E\OF
$((2 + 3)) $name
EOF
{ cat; cat <&3; } <<LEFT 3<<-"RIGHT"
left
LEFT
	right $name
	RIGHT
cat <<JOIN |
one\
two
JOIN
cat
cat <<-TABS
		line
 space	then tab
	TABS
cat <<'KEEP'
one\
two
KEEP
cat <<END
before\
END
after
END
v=$(cat <<INNER
nested-$name-$(echo cmd)
INNER
)
echo "$v"
again() {
    cat <<DATA
arg:$1:$name
DATA
}
again first
name=after
again second
read -r result <<INPUT
read $name
INPUT
echo "readback:$result"
: > side.tmp
false && cat <<UNUSED
$(echo wrong >> side.tmp) ${unbound:?unused}
UNUSED
if cat 2> err.tmp < NODATA.QQ <<SKIP; then echo wrong; fi
$(echo wrong >> side.tmp)
SKIP
cat side.tmp
sh large.sh > here.bin
echo "large:$?"
echo finished
exit 37
