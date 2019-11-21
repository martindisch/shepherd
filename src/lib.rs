//! A distributed video encoder that splits files into chunks for multiple
//! machines.

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
/// TODO: this is so short for testing, raise to 1 minute afterwards
const SEGMENT_LENGTH: Duration = Duration::from_secs(30);

/// The generic result type for this crate.
pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Starts the whole operation and cleans up afterwards.
///
/// # Arguments
/// * `input` - The path to the input file.
/// * `output` - The path to the output file.
/// * `hosts` - Comma-separated list of hosts.
pub fn run(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    hosts: Vec<&str>,
) -> Result<()> {
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
    let mut tmp_dir = dirs::home_dir().ok_or("Home directory not found")?;
    tmp_dir.push(TMP_DIR);
    fs::create_dir(&tmp_dir)?;

    // Start the operation
    let result = run_local(
        input.as_ref(),
        output.as_ref(),
        &tmp_dir,
        &hosts,
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
    local::split_video(input, &chunk_dir, SEGMENT_LENGTH, &running)?;
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
