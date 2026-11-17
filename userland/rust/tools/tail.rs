#![no_std]
use dosrs_user::{
    Args,
    ends::{self, End},
};
fn run(args: Args<'_>) -> i32 {
    ends::run(args, End::Tail)
}
dosrs_user::utility!(
    run,
    "tail",
    "tail [-qvz] [-n NUM | -c NUM] [FILE...]\nWrite the last lines or bytes.",
    0
);
