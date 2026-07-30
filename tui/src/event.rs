use std::sync::mpsc;
use std::time::Duration;
use std::{io, thread};

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, MouseEvent};

#[derive(Debug)]
pub enum Event {
    Key(KeyEvent),
    Mouse(MouseEvent),
    /// Bracketed paste: the pasted text arrives as one event instead of a key
    /// storm, and is routed to whichever text input is open.
    Paste(String),
    /// Terminal resize event. Ratatui re-queries the terminal size automatically,
    /// so no data is needed — this variant just triggers a redraw.
    Resize,
    Tick,
    /// An I/O error from the event polling thread.
    Error(io::Error),
}

/// Owns the receiving end of the event channel. The polling thread is detached:
/// it exits when the channel disconnects on drop, and in any case dies with the
/// process — nothing observable depends on it stopping first.
pub struct EventHandler {
    rx: mpsc::Receiver<Event>,
}

impl EventHandler {
    /// Spawn the event polling thread and return an `EventHandler` whose `next` method
    /// blocks until the next event arrives.
    pub fn new(tick_rate: Duration) -> Self {
        debug_assert!(!tick_rate.is_zero(), "tick_rate must be non-zero");

        let (event_tx, rx) = mpsc::channel();

        thread::spawn(move || {
            loop {
                let poll_result = match event::poll(tick_rate) {
                    Ok(ready) => ready,
                    Err(e) => {
                        if event_tx.send(Event::Error(e)).is_err() {
                            return;
                        }
                        return;
                    }
                };

                if poll_result {
                    let read_result = match event::read() {
                        Ok(ev) => ev,
                        Err(e) => {
                            if event_tx.send(Event::Error(e)).is_err() {
                                return;
                            }
                            return;
                        }
                    };

                    // Focus and other crossterm events are intentionally ignored.
                    let forwarded = match read_result {
                        CrosstermEvent::Key(key) => Some(Event::Key(key)),
                        CrosstermEvent::Mouse(mouse) => Some(Event::Mouse(mouse)),
                        CrosstermEvent::Paste(text) => Some(Event::Paste(text)),
                        CrosstermEvent::Resize(_, _) => Some(Event::Resize),
                        _ => None,
                    };
                    if let Some(event) = forwarded
                        && event_tx.send(event).is_err()
                    {
                        return;
                    }
                } else if event_tx.send(Event::Tick).is_err() {
                    return;
                }
            }
        });

        Self { rx }
    }

    /// Block until the next event is available.
    pub fn next(&self) -> Result<Event, mpsc::RecvError> {
        self.rx.recv()
    }
}
