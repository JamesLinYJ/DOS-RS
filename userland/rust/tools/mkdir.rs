#![no_std]
use dosrs_user::{Args, directories};
fn run(args: Args<'_>) -> i32 {
    directories::run(args, false)
}
dosrs_user::utility!(
    run,
    "mkdir",
    "mkdir [-pv] [-m MODE] DIRECTORY...\nCreate directories.",
    0
);
