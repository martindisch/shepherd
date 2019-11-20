use crossbeam::channel::{self, Receiver};
use std::{path::PathBuf, process::Command, thread};

static TMP_DIR: &str = "shepherd_tmp_remote";

/// The parent thread managing the operations for a host.
pub fn host_thread(host: String, global_receiver: Receiver<PathBuf>) {
    println!("Spawned host thread {}", host);

    // Create temporary directory on host
    let output = Command::new("ssh")
        .args(&[&host, "mkdir", TMP_DIR])
        .output()
        .expect("Failed executing ssh command");
    assert!(
        output.status.success(),
        "Failed creating remote temporary directory"
    );

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
        // Transfer chunk to host
        let output = Command::new("scp")
            .args(&[
                chunk.to_str().expect("Invalid Unicode"),
                &format!("{}:{}", host, TMP_DIR),
            ])
            .output()
            .expect("Failed executing scp command");
        assert!(output.status.success(), "Failed transferring chunk");
        // Pass the chunk to the encoder thread (blocks until channel empty)
        sender.send(chunk).expect("Failed sending chunk in channel");
    }
    // Since the global channel is empty, drop our sender to disconnect the
    // local channel
    drop(sender);
    println!("Host thread {} waiting for encoder to finish", host);

    // Wait for the encoder
    handle.join().expect("Encoder thread panicked");

    // Clean up temporary directory on host
    let output = Command::new("ssh")
        .args(&[&host, "rm", "-r", TMP_DIR])
        .output()
        .expect("Failed executing ssh command");
    assert!(
        output.status.success(),
        "Failed removing remote temporary directory"
    );
    println!("Host thread {} exiting", host);
}

/// The thread responsible for encoding files on a host.
pub fn encoder_thread(host: String, receiver: Receiver<PathBuf>) {
    while let Ok(chunk) = receiver.recv() {
        println!("Encoder thread {} received chunk {:?}", host, chunk);
        // Construct the chunk's remote file name
        let chunk_name = format!(
            "{}/{}",
            TMP_DIR,
            chunk
                .file_name()
                .expect("No normal file")
                .to_str()
                .expect("Invalid Unicode")
        );
        // Construct the encoded chunk's remote file name
        let enc_name = format!(
            "{}/{}.{}",
            TMP_DIR,
            chunk
                .file_stem()
                .expect("No normal file")
                .to_str()
                .expect("Invalid Unicode"),
            "mp4"
        );

        // Encode the chunk remotely
        let output = Command::new("ssh")
            .args(&[
                &host,
                "ffmpeg",
                "-i",
                &chunk_name,
                "-c:v",
                "libx264",
                "-crf",
                "22",
                "-preset",
                "veryslow",
                "-profile:v",
                "high",
                "-level",
                "4.2",
                "-pix_fmt",
                "yuv420p",
                &enc_name,
            ])
            .output()
            .expect("Failed executing ssh command");
        assert!(output.status.success(), "Failed encoding");
    }
    println!("Encoder thread {} exiting", host);
}
