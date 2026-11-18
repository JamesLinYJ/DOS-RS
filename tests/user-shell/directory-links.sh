# Btrfs-only supplement. The fixture supplies directory, dangling and looping
# symbolic links; no host pathname or external mkdir/ln utility is required.
root=$(pwd -P)
prefix=${root%/}
out=$prefix/linkop.out
err=$prefix/linkop.err
equal() {
    case "$1" in "$2") echo "$3";; *) echo "BAD:$3:[$1]:[$2]";; esac
}

cd alias
equal "$PWD" "$prefix/alias" link-logical-pwd
equal "$(pwd -L)" "$prefix/alias" link-logical-output
equal "$(pwd -P)" "$prefix/OTHER/INNER" link-physical-output
cd ..
equal "$PWD" "$root" link-logical-parent

cd -P alias
equal "$PWD" "$prefix/OTHER/INNER" link-physical-pwd
cd ..
equal "$PWD" "$prefix/OTHER" link-physical-parent
cd "$root"
cd -L alias/..
equal "$PWD" "$root" link-dotdot-logical
cd -P alias/..
equal "$PWD" "$prefix/OTHER" link-dotdot-physical
cd "$root"
cd -LP alias/..
equal "$PWD" "$prefix/OTHER" cd-last-physical
cd "$root"
cd -P -L alias/..
equal "$PWD" "$root" cd-last-logical

cd -P alias
export EXPECT_LOGICAL=$prefix/alias EXPECT_PHYSICAL=$prefix/OTHER/INNER
PWD=$EXPECT_LOGICAL sh -c '
    case "$PWD" in "$EXPECT_LOGICAL") echo initial-symlink-preserved;; *) echo BAD-initial-symlink;; esac
    case "$(pwd -P)" in "$EXPECT_PHYSICAL") echo initial-symlink-physical;; *) echo BAD-initial-physical;; esac
'
cd "$root"
CDPATH=up
cd portal > "$out"
unset CDPATH
equal "$PWD" "$prefix/up/portal" cdpath-link-logical
IFS= read -r line < "$out"
equal "$line" "$PWD" cdpath-link-output
equal "$(pwd -P)" "$prefix/OTHER/INNER" cdpath-link-physical
cd ..
equal "$PWD" "$prefix/up" cdpath-link-parent
cd "$root"

for invalid in dangling cycle; do
    (
        if cd "$invalid" > "$out" 2> "$err"; then echo "BAD-link-success:$invalid"; fi
        equal "$PWD" "$root" "link-pwd-preserved:$invalid"
        equal "$(pwd -P)" "$root" "link-cwd-preserved:$invalid"
        if IFS= read -r line < "$err"; then echo "link-diagnostic:$invalid"; else echo "BAD-link-error:$invalid"; fi
    )
done
exit 83
