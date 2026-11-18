#![no_std]
use dosrs_user::{Args, links};
fn run(args: Args<'_>) -> i32 {
    links::run(args, false)
}
dosrs_user::utility!(
    run,
    "link",
    "link FILE1 FILE2\nCreate a hard link to FILE1 named FILE2.",
    0
);
