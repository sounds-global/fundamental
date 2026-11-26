use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

const POLLS: usize = 64;

/// `future`'s answer when it gives one without waiting; polled directly so no runtime's
/// scheduler is counted.
pub(crate) fn ready<T>(future: impl Future<Output = T>) -> Option<T> {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    for _ in 0..POLLS {
        if let Poll::Ready(answer) = future.as_mut().poll(&mut context) {
            return Some(answer);
        }
    }
    None
}
