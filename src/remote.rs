use crossbeam::channel::{self, Receiver};
use std::{path::PathBuf, thread, time::Duration};

/// The parent thread managing the operations for a host.
pub fn host_thread(host: String, global_receiver: Receiver<PathBuf>) {
    println!("Spawned host thread {}", host);
    // Create a channel holding a single chunk at a time for the encoder thread
    let (sender, receiver) = channel::bounded(1);
    // Create copy of host for thread
    let host_cpy = host.clone();
    // Start the encoder thread
    let handle = thread::spawn(move || {
        encoder_thread(host_cpy, receiver);
    });

    // Try to fetch a chunk from the global channel
    while let Ok(chunk) = global_receiver.recv() {
        println!("Host thread {} received chunk {:?}", host, chunk);
        // TODO: transfer the chunk
        // Pass the chunk to the encoder thread (blocks until channel empty)
        sender.send(chunk).expect("Failed sending chunk in channel");
    }
    // Since the global channel is empty, drop our sender to disconnect the
    // local channel
    drop(sender);
    println!("Host thread {} waiting for encoder to finish", host);

    // Wait for the encoder
    handle
        .join()
        .expect("Encoder thread panicked");
    println!("Host thread {} exiting", host);
}

/// The thread responsible for encoding files on a host.
pub fn encoder_thread(host: String, receiver: Receiver<PathBuf>) {
    while let Ok(chunk) = receiver.recv() {
        println!("Encoder thread {} received chunk {:?}", host, chunk);
        // TODO: encode chunk
        thread::sleep(Duration::from_secs(2));
    }
    println!("Encoder thread {} exiting", host);
}
