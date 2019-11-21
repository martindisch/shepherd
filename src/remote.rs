use crossbeam::channel::{self, Receiver};
use log::{debug, info};
use std::{path::PathBuf, process::Command, thread};

static TMP_DIR: &str = "shepherd_tmp_remote";

/// The parent thread managing the operations for a host.
pub fn host_thread(
    host: String,
    global_receiver: Receiver<PathBuf>,
    encoded_dir: PathBuf,
) {
    debug!("Spawned host thread {}", host);

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
    let (sender, receiver) = channel::bounded(0);
    // Create copy of host for thread
    let host_cpy = host.clone();
    // Start the encoder thread
    let handle = thread::spawn(move || encoder_thread(host_cpy, receiver));

    // Try to fetch a chunk from the global channel
    while let Ok(chunk) = global_receiver.recv() {
        debug!("Host thread {} received chunk {:?}", host, chunk);
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
    debug!("Host thread {} waiting for encoder to finish", host);

    // Wait for the encoder
    let encoded = handle.join().expect("Encoder thread panicked");
    debug!("Host thread {} got encoded chunks {:?}", host, encoded);

    // Get a &str from encoded_dir PathBuf
    let encoded_dir = encoded_dir.to_str().expect("Invalid Unicode");
    // Transfer the encoded chunks back
    for chunk in &encoded {
        let output = Command::new("scp")
            .args(&[&format!("{}:{}", host, chunk), encoded_dir])
            .output()
            .expect("Failed executing scp command");
        assert!(output.status.success(), "Failed transferring encoded chunk");
        info!("{} returned encoded chunk {}", host, chunk);
    }

    // Clean up temporary directory on host
    let output = Command::new("ssh")
        .args(&[&host, "rm", "-r", TMP_DIR])
        .output()
        .expect("Failed executing ssh command");
    assert!(
        output.status.success(),
        "Failed removing remote temporary directory"
    );
    debug!("Host thread {} exiting", host);
}

/// Encodes chunks on a host and returns the encoded remote file names.
pub fn encoder_thread(
    host: String,
    receiver: Receiver<PathBuf>,
) -> Vec<String> {
    // We'll use this to store the encoded chunks' remote file names.
    let mut encoded = Vec::new();

    while let Ok(chunk) = receiver.recv() {
        debug!("Encoder thread {} received chunk {:?}", host, chunk);
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
        info!("{} starts encoding chunk {:?}", host, chunk);
        let output = Command::new("ssh")
            .args(&[
                &host,
                "ffmpeg",
                "-y",
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

        // Remember the encoded chunk
        encoded.push(enc_name);
    }
    debug!("Encoder thread {} exiting", host);

    encoded
}
