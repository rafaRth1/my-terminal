//! Bridges blocking `std::sync::mpsc` receivers into async streams for gpui.

use std::sync::mpsc::Receiver;

use futures::channel::mpsc::{UnboundedReceiver, unbounded};

/// Forwards every item of `source` to the returned stream on a named thread.
///
/// The thread ends when `source` disconnects or the stream is dropped.
pub fn to_stream<T: Send + 'static>(name: &str, source: Receiver<T>) -> UnboundedReceiver<T> {
    let (tx, rx) = unbounded();
    let spawned = std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            while let Ok(item) = source.recv() {
                if tx.unbounded_send(item).is_err() {
                    break;
                }
            }
        });
    if let Err(err) = spawned {
        eprintln!("my-terminal: cannot start {name} thread: {err}");
    }
    rx
}

#[cfg(test)]
mod tests {
    use futures::StreamExt as _;
    use futures::executor::block_on;

    use super::*;

    #[test]
    fn forwards_items_in_order_and_ends_with_source() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut stream = to_stream("test", rx);
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        drop(tx);
        let items: Vec<i32> = block_on(async {
            let mut items = Vec::new();
            while let Some(item) = stream.next().await {
                items.push(item);
            }
            items
        });
        assert_eq!(items, vec![1, 2]);
    }
}
