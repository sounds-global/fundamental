use fundamental_messages::Messages;
use fundamental_messages_fixture::m;

#[derive(Messages)]
#[messages(m::change)]
enum Change {
    ChangedAt { at: String },
    Counted { noun: String, count: u32 },
    AnyValue { field: String },
}

fn main() {}
