root=$(pwd -P)
prefix=${root%/}
out=$prefix/dirop.out
err=$prefix/dirop.err
export EXPECT_ROOT=$root
export EXPECT_ALIAS=$root//
equal() {
    case "$1" in "$2") echo "$3";; *) echo "BAD:$3:[$1]:[$2]";; esac
}
silent() {
    if IFS= read -r line < "$out"; then echo "BAD-output:$1:$line"; else echo "$1"; fi
}
reject_cd() {
    reason=$1
    shift
    prior_physical=$(pwd -P)
    prior_logical=$PWD
    if cd "$@" > "$out" 2> "$err"; then echo "BAD-success:$reason"; fi
    equal "$(pwd -P)" "$prior_physical" "unchanged:$reason"
    equal "$PWD" "$prior_logical" "pwd-preserved:$reason"
    if IFS= read -r line < "$err"; then echo "diagnostic:$reason"; else echo "BAD-no-error:$reason"; fi
}

equal "$PWD" "$root" initial-pwd
(unset PWD; sh -c 'case "$PWD" in "$EXPECT_ROOT") echo initial-unset;; *) echo BAD-initial-unset;; esac')
for candidate in '' . "$prefix/SUB" "$root/." "$prefix/SUB/.."; do
    PWD=$candidate sh -c 'case "$PWD" in "$EXPECT_ROOT") echo initial-repaired;; *) echo BAD-initial-repaired;; esac'
done
PWD=$EXPECT_ALIAS sh -c '
    case "$PWD" in "$EXPECT_ALIAS") echo initial-logical;; *) echo BAD-initial-logical;; esac
    case "$(pwd)" in "$EXPECT_ALIAS") echo pwd-default-logical;; *) echo BAD-pwd-default;; esac
    case "$(pwd -L)" in "$EXPECT_ALIAS") echo pwd-logical;; *) echo BAD-pwd-logical;; esac
    case "$(pwd -P)" in "$EXPECT_ROOT") echo pwd-physical;; *) echo BAD-pwd-physical;; esac
    case "$(pwd -LP)" in "$EXPECT_ROOT") echo pwd-last-physical;; *) echo BAD-pwd-last-P;; esac
    case "$(pwd -P -L)" in "$EXPECT_ALIAS") echo pwd-last-logical;; *) echo BAD-pwd-last-L;; esac
'

cd SUB
equal "$PWD" "$prefix/SUB" changed-logical
equal "$OLDPWD" "$root" old-working-directory
export EXPECT_SUB=$PWD
sh -c 'case "$PWD" in "$EXPECT_SUB") echo child-pwd-exported;; *) echo BAD-child-pwd;; esac'
cd ../SUB/../.
equal "$PWD" "$root" canonical-parent
saved_home=$HOME
HOME=$prefix/SUB
cd
HOME=$saved_home
equal "$PWD" "$prefix/SUB" home-directory
cd - > "$out"
IFS= read -r line < "$out"
equal "$line" "$root" previous-output
equal "$PWD" "$root" previous-directory

CDPATH=absent:OTHER:LOOK
cd INNER > "$out"
unset CDPATH
equal "$PWD" "$prefix/OTHER/INNER" cdpath-first-directory
IFS= read -r line < "$out"
equal "$line" "$PWD" cdpath-output
cd "$root"
CDPATH=:OTHER
cd SUB > "$out"
unset CDPATH
equal "$PWD" "$prefix/SUB" cdpath-empty-component
silent cdpath-empty-silent
cd "$root"
CDPATH=.
cd SUB > "$out"
unset CDPATH
IFS= read -r line < "$out"
equal "$line" "$PWD" cdpath-dot-output
cd "$root"
CDPATH=OTHER
cd ./SUB > "$out"
unset CDPATH
equal "$PWD" "$prefix/SUB" cdpath-dot-bypass
silent cdpath-dot-bypass-silent
cd "$root"
CDPATH=OTHER
cd "$prefix/SUB" > "$out"
unset CDPATH
equal "$PWD" "$prefix/SUB" cdpath-absolute-bypass
silent cdpath-absolute-silent
cd "$root"
CDPATH=absent
cd SUB > "$out"
unset CDPATH
equal "$PWD" "$prefix/SUB" cdpath-fallback
silent cdpath-fallback-silent
cd "$root"
CDPATH=SUB/ONE.TX:OTHER
cd INNER > "$out"
unset CDPATH
equal "$PWD" "$prefix/OTHER/INNER" cdpath-not-directory-skipped
cd "$root"

# Isolate negative cases so a reference shell's successful extension cannot
# change the starting directory for the remaining cases.
(reject_cd missing-parent absent/..)
(reject_cd file-parent SUB/ONE.TX/..)
(reject_cd regular-file SUB/ONE.TX)
(reject_cd empty '')
(reject_cd operands SUB OTHER)
(reject_cd option -Q)
# These four cases additionally pin DOS-RS policy where the standard leaves
# the environment's contents or application changes to PWD unspecified.
(unset HOME; reject_cd home)
(unset OLDPWD; reject_cd previous -)
(readonly PWD; reject_cd readonly-pwd SUB)
(readonly OLDPWD; reject_cd readonly-oldpwd SUB)

(
cd -- -dir
equal "$PWD" "$prefix/-dir" option-like-directory
)
(
cd SUB
cd -Pe "$root"
equal "$PWD" "$root" physical-e-success
)
(
cd -e SUB
equal "$PWD" "$prefix/SUB" logical-e-ignored
)
equal "$(pwd --)" "$root" pwd-delimiter
for argument in -Q operand; do
    if pwd "$argument" > "$out" 2> "$err"; then echo BAD-pwd-error; fi
    silent pwd-error-silent
done

i=0
while case $i in 64) false;; *) true;; esac; do
    cd SUB
    cd ..
    i=$((i + 1))
done
equal "$PWD" "$root" repeated-directory-state
exit 79
