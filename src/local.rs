use std::{path::Path, process::Command};

use super::Result;

/// Uses `ffmpeg` to locally extract and encode the audio.
pub fn extract_audio(input: &Path, output: &Path) -> Result<()> {
    // Convert input and output to &str
    let input = input.to_str().ok_or("Input invalid Unicode")?;
    let output = output.to_str().ok_or("Output invalid Unicode")?;
    // Do the extraction
    let output = Command::new("ffmpeg")
        .args(&["-i", input, "-vn", "-c:a", "aac", "-b:a", "192k", output])
        .output()?;
    if !output.status.success() {
        return Err("Failed extracting audio".into());
    }

    Ok(())
}
