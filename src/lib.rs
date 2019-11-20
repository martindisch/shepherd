use dirs;
use std::{error::Error, fs, path::Path, time::Duration};

mod local;

/// The name of the temporary directory in the home directory to collect
/// intermediate files.
const TMP_DIR: &str = "shepherd_tmp";
/// The name of the encoded audio track.
const AUDIO: &str = "audio.aac";
/// The length of chunks to split the video into.
const SEGMENT_LENGTH: Duration = Duration::from_secs(30);

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

    Ok(())
}
