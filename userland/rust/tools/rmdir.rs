#![no_std]
use dosrs_user::{Args, directories};
fn run(args: Args<'_>) -> i32 {
    directories::run(args, true)
}
dosrs_user::utility!(
    run,
    "rmdir",
    "rmdir [-pv] DIRECTORY...\nRemove empty directories.",
    0
);
