//! A queue / stream / consumer name is a single flat token, not a dotted subject — a `.` in a
//! `name = "…"` override must not compile. Expected to fail; the paired `.stderr` snapshot pins
//! the diagnostics (which suggest `_` instead of `.`).

use fundamental_nats::{Consumer, Queue, Stream};

#[derive(Queue)]
#[queue(name = "layer5.erase")]
struct Layer5Erase;

#[derive(Stream)]
#[stream(name = "orders.v2")]
struct Orders;

#[derive(Consumer)]
#[consumer(name = "proc.main")]
struct Proc;

fn main() {}
