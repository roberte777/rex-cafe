//! What reaches the Café from outside, whether from Rex or the demo.

use crate::agents::Fact;

#[derive(Clone, Debug, PartialEq)]
pub enum FeedMsg {
    /// Connected to Rex: the Barista is in.
    Open,
    /// Lost Rex: the café is Closed until it reconnects.
    Closed,
    /// A fresh snapshot that replaces everything known so far.
    Reset(Vec<Fact>),
    Fact(Fact),
    /// Something worth telling the user briefly.
    Notice(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Jump { session: String, block: String },
    Spawn,
    Resync,
}
