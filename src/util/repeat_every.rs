use std::ops::ControlFlow;
use std::time::Duration;

use crownui::prelude::Cx;

/// Calls `tick` on the UI thread every `period` until it breaks.
pub fn repeat_every(
    cx: &mut Cx,
    period: Duration,
    mut tick: impl FnMut(&mut Cx) -> ControlFlow<()> + 'static,
) {
    cx.spawn(
        async move { tokio::time::sleep(period).await },
        move |cx, ()| {
            if tick(cx).is_continue() {
                repeat_every(cx, period, tick);
            }
        },
    );
}
