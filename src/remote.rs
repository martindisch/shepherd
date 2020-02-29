//! Functions for operations on remote hosts.

use crossbeam::channel::{self, Receiver};
use log::{debug, info};
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    thread,
};

/// The name of the temporary directory in the home directory of remote hosts.
pub static TMP_DIR: &str = "shepherd_tmp_remote";

/// The parent thread managing the operations for a host.
pub fn host_thread(
    host: String,
    global_receiver: Receiver<PathBuf>,
    encoded_dir: PathBuf,
    out_ext: String,
    running: Arc<AtomicBool>,
) {
    debug!("Spawned host thread {}", host);

    // Clean up temporary directory on host. This is necessary, because it's
    // possible that the user ran with the --keep flag before. While our
    // application wouldn't get confused by old chunks lying around (they're
    // overwritten and those that aren't are disregarded, because it keeps
    // track of its chunks), we don't want the user to get the wrong idea.
    // Also, we don't care if this fails, because if it did then the directory
    // didn't exist anyway.
    Command::new("ssh")
        .args(&[&host, "rm", "-r", crate::remote::TMP_DIR])
        .output()
        .expect("Failed executing ssh command");

    // Create temporary directory on host
    let output = Command::new("ssh")
        .args(&[&host, "mkdir", TMP_DIR])
        .output()
        .expect("Failed executing ssh command");
    assert!(
        output.status.success() || !running.load(Ordering::SeqCst),
        "Failed creating remote temporary directory"
    );

    // Create a channel holding a single chunk at a time for the encoder thread
    let (sender, receiver) = channel::bounded(0);
    // Create copy of host for thread
    let host_cpy = host.clone();
    // Create copy of running indicator for thread
    let r = running.clone();
    // Start the encoder thread
    let handle = thread::Builder::new()
        .name(format!("{}-encoder", host))
        .spawn(move || encoder_thread(host_cpy, out_ext, receiver, r))
        .expect("Failed spawning thread");

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
        assert!(
            output.status.success() || !running.load(Ordering::SeqCst),
            "Failed transferring chunk"
        );

        // Pass the chunk to the encoder thread (blocks until encoder is ready
        // to receive and fails if it terminated prematurely)
        if sender.send(chunk).is_err() {
            // Encoder stopped, so quit early
            break;
        }
    }
    // Since the global channel is empty, drop our sender to disconnect the
    // local channel
    drop(sender);
    debug!("Host thread {} waiting for encoder to finish", host);

    // Wait for the encoder
    let encoded = handle.join().expect("Encoder thread panicked");
    // Abort early if signal was sent
    if !running.load(Ordering::SeqCst) {
        info!("{} exiting", host);
        return;
    }
    debug!("Host thread {} got encoded chunks {:?}", host, encoded);

    // Get a &str from encoded_dir PathBuf
    let encoded_dir = encoded_dir.to_str().expect("Invalid Unicode");
    // Transfer the encoded chunks back
    for chunk in &encoded {
        let output = Command::new("scp")
            .args(&[&format!("{}:{}", host, chunk), encoded_dir])
            .output()
            .expect("Failed executing scp command");
        assert!(
            output.status.success() || !running.load(Ordering::SeqCst),
            "Failed transferring encoded chunk"
        );
        info!("{} returned encoded chunk {}", host, chunk);
    }

    debug!("Host thread {} exiting", host);
}

/// Encodes chunks on a host and returns the encoded remote file names.
fn encoder_thread(
    host: String,
    out_ext: String,
    receiver: Receiver<PathBuf>,
    running: Arc<AtomicBool>,
) -> Vec<String> {
    // We'll use this to store the encoded chunks' remote file names.
    let mut encoded = Vec::new();

    while let Ok(chunk) = receiver.recv() {
        // Abort early if signal was sent
        if !running.load(Ordering::SeqCst) {
            break;
        }

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
            "{}/enc_{}.{}",
            TMP_DIR,
            chunk
                .file_stem()
                .expect("No normal file")
                .to_str()
                .expect("Invalid Unicode"),
            out_ext
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
                "26",
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
        assert!(
            output.status.success() || !running.load(Ordering::SeqCst),
            "Failed encoding"
        );

        // Remember the encoded chunk
        encoded.push(enc_name);
    }
    debug!("Encoder thread {} exiting", host);

    encoded
}
