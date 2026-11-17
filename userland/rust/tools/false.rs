#![no_std]
use dosrs_user::Args;
fn run(_: Args<'_>) -> i32 {
    1
}
dosrs_user::utility!(
    run,
    "false",
    "false\nReturn an unsuccessful exit status.",
    1
);
