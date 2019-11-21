//! Functions for operations on the local host.

use std::{fs, path::Path, process::Command, time::Duration};

use super::Result;

/// Uses `ffmpeg` to locally extract and encode the audio.
pub fn extract_audio(input: &Path, output: &Path) -> Result<()> {
    // Convert input and output to &str
    let input = input.to_str().ok_or("Input invalid Unicode")?;
    let output = output.to_str().ok_or("Output invalid Unicode")?;
    // Do the extraction
    let output = Command::new("ffmpeg")
        .args(&[
            "-y", "-i", input, "-vn", "-c:a", "aac", "-b:a", "192k", output,
        ])
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
            "-y",
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

/// Uses `ffmpeg` to locally combine the encoded chunks and audio.
pub fn combine(encoded_dir: &Path, audio: &Path, output: &Path) -> Result<()> {
    // Create list of encoded chunks
    let mut chunks = fs::read_dir(&encoded_dir)?
        .map(|res| res.and_then(|readdir| Ok(readdir.path())))
        .map(|res| res.map_err(|e| e.into()))
        .map(|res| res.and_then(|path| Ok(path.into_os_string())))
        .map(|res| {
            res.and_then(|os_string| {
                os_string
                    .into_string()
                    .map_err(|_| "Failed OsString conversion".into())
            })
        })
        .map(|res| res.and_then(|file| Ok(format!("file '{}'\n", file))))
        .collect::<Result<Vec<String>>>()?;
    // Sort them so we have the right order
    chunks.sort();
    // And finally join them
    let chunks = chunks.join("");
    // Now write that to a file
    let mut file_list = encoded_dir.to_path_buf();
    file_list.push("files.txt");
    fs::write(&file_list, chunks)?;

    // Convert paths to &str
    let audio = audio.to_str().ok_or("Audio invalid Unicode")?;
    let file_list = file_list.to_str().ok_or("File list invalid Unicode")?;
    let output = output.to_str().ok_or("Output invalid Unicode")?;
    // Combine everything
    let output = Command::new("ffmpeg")
        .args(&[
            "-y",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            file_list,
            "-i",
            audio,
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            output,
        ])
        .output()?;
    if !output.status.success() {
        return Err("Failed combining video".into());
    }

    Ok(())
}
