use std::sync::mpsc;
use std::time::Duration;
use std::{io, thread};

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};

#[derive(Debug)]
pub enum Event {
    Key(KeyEvent),
    /// Terminal resize event. The dimensions are forwarded from crossterm but
    /// ratatui re-queries the terminal size automatically, so the values are
    /// intentionally unused here.
    #[allow(dead_code)]
    Resize(u16, u16),
    Tick,
    /// An I/O error from the event polling thread.
    Error(io::Error),
}

pub struct EventHandler {
    rx: mpsc::Receiver<Event>,
    /// Keep the sender alive so the spawned thread does not detect a disconnected channel
    /// prematurely and exit before `EventHandler` is dropped.
    _tx: mpsc::Sender<Event>,
}

impl EventHandler {
    /// Spawn the event polling thread and return an `EventHandler` whose `next` method
    /// blocks until the next event arrives.
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::channel();
        let event_tx = tx.clone();

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

                    match read_result {
                        CrosstermEvent::Key(key) => {
                            if event_tx.send(Event::Key(key)).is_err() {
                                return;
                            }
                        }
                        CrosstermEvent::Resize(w, h) => {
                            if event_tx.send(Event::Resize(w, h)).is_err() {
                                return;
                            }
                        }
                        _ => {}
                    }
                } else if event_tx.send(Event::Tick).is_err() {
                    return;
                }
            }
        });

        Self { rx, _tx: tx }
    }

    /// Block until the next event is available.
    pub fn next(&self) -> Result<Event, mpsc::RecvError> {
        self.rx.recv()
    }
}
