#![no_std]
use dosrs_user::{Args, links};
fn run(args: Args<'_>) -> i32 {
    links::run(args, true)
}
dosrs_user::utility!(
    run,
    "unlink",
    "unlink FILE\nRemove the directory entry for FILE.",
    0
);
