#![no_std]
use dosrs_user::Args;
fn run(_: Args<'_>) -> i32 {
    0
}
dosrs_user::utility!(run, "true", "true\nReturn a successful exit status.", 0);
