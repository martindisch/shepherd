use crossbeam::channel::Receiver;
use std::{path::PathBuf, thread, time::Duration};

/// The parent thread managing the operations for a host.
pub fn host_thread(host: String, receiver: Receiver<PathBuf>) {
    println!("Spawned thread for host {}", host);
    while let Ok(chunk) = receiver.recv() {
        println!("Thread for host {} received chunk {:?}", host, chunk);
        thread::sleep(Duration::from_secs(1));
    }
    println!("Thread for host {} done", host);
}
