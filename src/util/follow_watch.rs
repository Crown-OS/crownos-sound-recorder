use crownui::prelude::{Cx, Signal};
use tokio::sync::watch;

/// Keeps `signal` equal to the latest value `receiver` sees, re-arming one `cx.spawn` per
/// change, until the sender or the signal is gone.
pub fn follow_watch<T: Clone + Send + Sync + 'static>(
    cx: &mut Cx,
    signal: Signal<T>,
    mut receiver: watch::Receiver<T>,
) {
    let next_change = async move {
        let open = receiver.changed().await.is_ok();
        (receiver, open)
    };
    cx.spawn(next_change, move |cx, (receiver, open)| {
        let latest = receiver.borrow().clone();
        if open && signal.try_set(cx, latest) {
            follow_watch(cx, signal, receiver);
        }
    });
}
