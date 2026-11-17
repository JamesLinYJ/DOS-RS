#![no_std]
use dosrs_user::{
    Args,
    ends::{self, End},
};
fn run(args: Args<'_>) -> i32 {
    ends::run(args, End::Head)
}
dosrs_user::utility!(
    run,
    "head",
    "head [-qvz] [-n NUM | -c NUM] [FILE...]\nWrite the first lines or bytes.",
    0
);
