//! A distributed video encoder that splits files into chunks to encode them on
//! multiple machines in parallel.
//!
//! ## Usage
//!
//! The prerequisites are one or more (you'll want more) computers—which we'll
//! refer to as hosts—with `ffmpeg` installed, configured such that you can SSH
//! into them using just their hostnames. In practice, this means you'll have
//! to set up your `.ssh/config` and `ssh-copy-id` your public key to the
//! machines. I only tested it on Linux, but if you manage to set up `ffmpeg`
//! and SSH, it might work on macOS or Windows directly or with little
//! modification.
//!
//! The usage is pretty straightforward:
//! ```text
//! USAGE:
//!     shepherd [OPTIONS] <IN> <OUT> --clients <hostnames>
//!
//! FLAGS:
//!     -h, --help       Prints help information
//!     -V, --version    Prints version information
//!
//! OPTIONS:
//!     -c, --clients <hostnames>    Comma-separated list of encoding hosts
//!     -l, --length <seconds>       The length of video chunks in seconds
//!     -t, --tmp <path>             The path to the local temporary directory
//!
//! ARGS:
//!     <IN>     The original video file
//!     <OUT>    The output video file
//! ```
//!
//! So if we have three machines c1, c2 and c3, we could do
//! ```console
//! $ shepherd -c c1,c2,c3 -l 30 source_file.mxf output_file.mp4
//! ```
//! to have it split the video in roughly 30 second chunks and encode them in
//! parallel.
//!
//! ## How it works
//!
//! 1. Creates a temporary directory in your home directory.
//! 2. Extracts the audio and encodes it. This is not parallelized, but the
//!    time this takes is negligible compared to the video anyway.
//! 3. Splits the video into chunks. This can take relatively long, since
//!    you're basically writing the full file to disk again. It would be nice
//!    if we could read chunks of the file and directly transfer them to the
//!    hosts, but that might be tricky with `ffmpeg`.
//! 4. Spawns a manager and an encoder thread for every host. The manager
//!    creates a temporary directory in the home directory of the remote and
//!    makes sure that the encoder always has something to encode. It will
//!    transfer a chunk, give it to the encoder to work on and meanwhile
//!    transfer another chunk, so the encoder can start directly with that once
//!    it's done, without wasting any time. But it will keep at most one chunk
//!    in reserve, to prevent the case where a slow machine takes too many
//!    chunks and is the only one still encoding while the faster ones are
//!    already done.
//! 5. When an encoder is done and there are no more chunks to work on, it will
//!    quit and the manager transfers the encoded chunks back before
//!    terminating itself.
//! 6. Once all encoded chunks have arrived, they're concatenated and the audio
//!    stream added.
//! 7. All remote and the local temporary directory are removed.
//!
//! Thanks to the work stealing method of distribution, having some hosts that
//! are significantly slower than others does not necessarily delay the overall
//! operation. In the worst case, the slowest machine is the last to start
//! encoding a chunk and remains the only working encoder for the duration it
//! takes to encode this one chunk. This window can easily be reduced by using
//! smaller chunks.
//!
//! ## Limitations
//!
//! Currently this is tailored to my use case, which is encoding large MXF
//! files containing DNxHD to MP4s with H.264 and AAC streams. It's fairly easy
//! to change the `ffmpeg` commands to support other formats though, and in
//! fact making it so that the user can supply arbitrary arguments to the
//! `ffmpeg` processing through the CLI is a pretty low-hanging fruit and I'm
//! interested in that if I find the time.
//!
//! As with all things parallel, Amdahl's law hits hard and you don't get twice
//! the speed with twice the processing power. With this approach, you pay for
//! having to split the video into chunks before you begin, transferring them
//! to the encoders and the results back, and reassembling them. But if your
//! system I/O (both disk and network) is good and you spend a lot of time
//! encoding (e.g. slow preset for H.264), it's still worth it.

use crossbeam::channel;
use dirs;
use log::{error, info};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    thread,
    time::Duration,
};

mod local;
mod remote;

/// The name of the temporary directory in the home directory to collect
/// intermediate files.
const TMP_DIR: &str = "shepherd_tmp";
/// The name of the encoded audio track.
const AUDIO: &str = "audio.aac";
/// The length of chunks to split the video into.
const DEFAULT_LENGTH: &str = "60";

/// The generic result type for this crate.
pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Starts the whole operation and cleans up afterwards.
///
/// # Arguments
/// * `input` - The path to the input file.
/// * `output` - The path to the output file.
/// * `hosts` - Comma-separated list of hosts.
/// * `seconds` - The video chunk length.
/// * `tmp_dir` - The path to the local temporary directory.
pub fn run(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    hosts: Vec<&str>,
    seconds: Option<&str>,
    tmp_dir: Option<&str>,
) -> Result<()> {
    // Convert the length
    let seconds = seconds.unwrap_or(DEFAULT_LENGTH).parse::<u64>()?;
    // Convert the tmp_dir
    let mut tmp_dir = tmp_dir
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .ok_or("Home directory not found")?;

    // Set up a shared boolean to check whether the user has aborted
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
        info!(
            "Abort signal received. Waiting for remote encoders to finish the \
             current chunk and quit gracefully."
        );
    })
    .expect("Error setting Ctrl-C handler");

    // Create our local temporary directory
    tmp_dir.push(TMP_DIR);
    fs::create_dir(&tmp_dir)?;

    // Start the operation
    let result = run_local(
        input.as_ref(),
        output.as_ref(),
        &tmp_dir,
        &hosts,
        seconds,
        running.clone(),
    );

    info!("Cleaning up");
    // Remove remote temporary directories
    for &host in &hosts {
        // Clean up temporary directory on host
        let output = Command::new("ssh")
            .args(&[&host, "rm", "-r", remote::TMP_DIR])
            .output()
            .expect("Failed executing ssh command");
        // These checks for `running` are necessary, because Ctrl + C also
        // seems to terminate the commands we launch, which means they'll
        // return unsuccessfully. With this check we prevent an error message
        // in this case, because that's what the user wants. Unfortunately this
        // also means we have to litter the `running` variable almost
        // everyhwere.
        if !output.status.success() && running.load(Ordering::SeqCst) {
            error!("Failed removing remote temporary directory on {}", host);
        }
    }
    // Remove local temporary directory
    fs::remove_dir_all(&tmp_dir).ok();

    result
}

/// Does the actual work.
///
/// This is separate so it can fail and return early, since cleanup is then
/// handled in its caller function.
fn run_local(
    input: &Path,
    output: &Path,
    tmp_dir: &Path,
    hosts: &[&str],
    seconds: u64,
    running: Arc<AtomicBool>,
) -> Result<()> {
    // Build path to audio file
    let mut audio = tmp_dir.to_path_buf();
    audio.push(AUDIO);
    // Start the extraction
    info!("Extracting audio");
    local::extract_audio(input, &audio, &running)?;

    // We check whether the user has aborted before every time-intensive task.
    // It's a better experience, but a bit ugly code-wise.
    if !running.load(Ordering::SeqCst) {
        // Abort early
        return Ok(());
    }

    // Create directory for video chunks
    let mut chunk_dir = tmp_dir.to_path_buf();
    chunk_dir.push("chunks");
    fs::create_dir(&chunk_dir)?;
    // Split the video
    info!("Splitting video into chunks");
    local::split_video(
        input,
        &chunk_dir,
        Duration::from_secs(seconds),
        &running,
    )?;
    // Get the list of created chunks
    let mut chunks = fs::read_dir(&chunk_dir)?
        .map(|res| res.and_then(|readdir| Ok(readdir.path())))
        .collect::<std::io::Result<Vec<PathBuf>>>()?;
    // Sort them so they're in order. That's not strictly necessary, but nicer
    // for the user to watch since it allows seeing the progress at a glance.
    chunks.sort();

    if !running.load(Ordering::SeqCst) {
        // Abort early
        return Ok(());
    }

    // Initialize the global channel for chunks
    let (sender, receiver) = channel::unbounded();
    // Send all chunks into it
    for chunk in chunks {
        sender.send(chunk)?;
    }
    // Drop the sender so the channel gets disconnected
    drop(sender);

    // Create directory for encoded chunks
    let mut encoded_dir = tmp_dir.to_path_buf();
    encoded_dir.push("encoded");
    fs::create_dir(&encoded_dir)?;
    // Spawn threads for hosts
    info!("Starting remote encoding");
    let mut host_threads = Vec::with_capacity(hosts.len());
    for &host in hosts {
        // Create owned hostname to move into the thread
        let host = host.to_string();
        // Clone the queue receiver for the thread
        let thread_receiver = receiver.clone();
        // Create owned encoded_dir for the thread
        let enc = encoded_dir.clone();
        // Create copy of running indicator for the thread
        let r = running.clone();
        // Start it
        let handle =
            thread::Builder::new().name(host.clone()).spawn(|| {
                remote::host_thread(host, thread_receiver, enc, r);
            })?;
        host_threads.push(handle);
    }

    // Wait for all hosts to finish
    for handle in host_threads {
        if handle.join().is_err() {
            return Err("A host thread panicked".into());
        }
    }

    if !running.load(Ordering::SeqCst) {
        // We aborted early
        return Ok(());
    }

    // Combine encoded chunks and audio
    info!("Combining encoded chunks into final video");
    local::combine(&encoded_dir, &audio, output, &running)?;

    Ok(())
}
