use std::{path::Path, process::Command, time::Duration};

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

/// Uses `ffmpeg` to locally split the video into chunks.
pub fn split_video(
    input: &Path,
    output_dir: &Path,
    segment_length: Duration,
) -> Result<()> {
    // Convert input and output to &str
    let input = input.to_str().ok_or("Input invalid Unicode")?;
    let mut output_dir = output_dir.to_path_buf();
    output_dir.push("chunk_%03d.mxf");
    let output = output_dir.to_str().ok_or("Output invalid Unicode")?;
    // Do the chunking
    let output = Command::new("ffmpeg")
        .args(&[
            "-i",
            input,
            "-an",
            "-c",
            "copy",
            "-f",
            "segment",
            "-segment_time",
            &segment_length.as_secs().to_string(),
            output,
        ])
        .output()?;
    if !output.status.success() {
        return Err("Failed splitting video".into());
    }

    Ok(())
}
