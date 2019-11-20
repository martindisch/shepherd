use crossbeam::channel;
use dirs;
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
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
const SEGMENT_LENGTH: Duration = Duration::from_secs(10);

/// The generic result type for this crate.
pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Starts the whole operation and cleans up afterwards.
pub fn run(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    hosts: Vec<&str>,
) -> Result<()> {
    // Create our local temporary directory
    let mut tmp_dir = dirs::home_dir().ok_or("Home directory not found")?;
    tmp_dir.push(TMP_DIR);
    fs::create_dir(&tmp_dir)?;

    // Start the operation
    let result = run_local(input.as_ref(), output.as_ref(), &tmp_dir, hosts);

    // Clean up infallibly
    // TODO: make sure this also happens when stopped with Ctrl + C
    fs::remove_dir_all(&tmp_dir).ok();

    result
}

/// Does the actual work.
///
/// This is separate so it can fail and return early, since cleanup is then
/// handled in its caller function.
pub fn run_local(
    input: &Path,
    output: &Path,
    tmp_dir: &Path,
    hosts: Vec<&str>,
) -> Result<()> {
    // Build path to audio file
    let mut audio = tmp_dir.to_path_buf();
    audio.push(AUDIO);
    // Start the extraction
    println!("Extracting audio");
    local::extract_audio(input, &audio)?;

    // Create directory for video chunks
    let mut chunk_dir = tmp_dir.to_path_buf();
    chunk_dir.push("chunks");
    fs::create_dir(&chunk_dir)?;
    // Split the video
    println!("Splitting video into chunks");
    local::split_video(input, &chunk_dir, SEGMENT_LENGTH)?;
    // Get the list of created chunks
    let chunks = fs::read_dir(&chunk_dir)?
        .map(|res| res.and_then(|readdir| Ok(readdir.path())))
        .collect::<std::io::Result<Vec<PathBuf>>>()?;

    // Initialize the global channel for chunks
    let (sender, receiver) = channel::unbounded();
    // Send all chunks into it
    for chunk in chunks {
        sender.send(chunk)?;
    }
    // Drop the sender so the channel gets disconnected
    drop(sender);

    // Spawn threads for hosts
    let mut host_threads = Vec::with_capacity(hosts.len());
    for &host in &hosts {
        // Create owned hostname to move into the thread
        let host = host.to_string();
        // Clone the queue receiver for the thread
        let thread_receiver = receiver.clone();
        // Start it
        let handle = thread::spawn(|| {
            remote::host_thread(host, thread_receiver);
        });
        host_threads.push(handle);
    }

    // Wait for all hosts to finish
    for handle in host_threads {
        if let Err(e) = handle.join() {
            println!("Thread for a host panicked: {:?}", e);
        }
    }

    Ok(())
}
