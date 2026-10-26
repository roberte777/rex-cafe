//! Plays the demo script on a fixed clock, for snapshot tests and `--dump`.

use crate::app::Model;
use crate::demo;
use crate::feed::FeedMsg;

/// The pretend wall clock at the start of a replay.
pub const WALL: f64 = 1_800_000_000.0;
const DT: f32 = 1.0 / 30.0;

/// Steps the demo forward and calls `at` at each requested time, in order.
pub fn replay(times: &[f64], mut at: impl FnMut(f64, &Model)) {
    let (opening, events) = demo::script(WALL);
    let mut model = Model::default();
    model.apply(FeedMsg::Reset(opening));
    model.apply(FeedMsg::Open);
    let mut events = events.into_iter().peekable();
    let mut checkpoints = times.iter().copied().peekable();
    let mut t = 0.0f64;
    while let Some(&next) = checkpoints.peek() {
        while events.peek().is_some_and(|(et, _)| *et <= t) {
            let (_, f) = events.next().unwrap();
            model.apply(FeedMsg::Fact(f));
        }
        if t + 1e-9 >= next {
            at(next, &model);
            checkpoints.next();
            continue;
        }
        model.cafe.update(DT);
        t += DT as f64;
    }
}
