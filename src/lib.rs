//! A distributed video encoder that splits files into chunks to encode them on
//! multiple machines in parallel.
//!
//! ## Installation
//!
//! Using Cargo, you can do
//! ```console
//! $ cargo install shepherd
//! ```
//! or just clone the repository and compile the binary with
//! ```console
//! $ git clone https://github.com/martindisch/shepherd
//! $ cd shepherd
//! $ cargo build --release
//! ```
//! There's also a
//! [direct download](https://github.com/martindisch/shepherd/releases/latest/download/shepherd)
//! for the latest x86-64 ELF binary.
//!
//! ## Usage
//!
//! The prerequisites are one or more (you'll want more) computers—which we'll
//! refer to as hosts—with `ffmpeg` installed and configured such that you can
//! SSH into them directly. This means you'll have to `ssh-copy-id` your public
//! key to them. I only tested it on Linux, but if you manage to set up
//! `ffmpeg` and SSH, it might work on macOS or Windows directly or with little
//! modification.
//!
//! The usage is pretty straightforward:
//! ```text
//! USAGE:
//!     shepherd [FLAGS] [OPTIONS] <IN> <OUT> --clients <hostnames> [FFMPEG OPTIONS]...
//!
//! FLAGS:
//!     -h, --help       Prints help information
//!     -k, --keep       Don't clean up temporary files
//!     -V, --version    Prints version information
//!
//! OPTIONS:
//!     -c, --clients <hostnames>    Comma-separated list of encoding hosts
//!     -l, --length <seconds>       The length of video chunks in seconds
//!     -t, --tmp <path>             The path to the local temporary directory
//!
//! ARGS:
//!     <IN>                   The original video file
//!     <OUT>                  The output video file
//!     <FFMPEG OPTIONS>...    Options/flags for ffmpeg encoding of chunks. The
//!                            chunks are video only, so don't pass in anything
//!                            concerning audio. Input/output file names are added
//!                            by the application, so there is no need for that
//!                            either. This is the last positional argument and
//!                            needs to be preceeded by double hypens (--) as in:
//!                            shepherd -c c1,c2 in.mp4 out.mp4 -- -c:v libx264
//!                            -crf 26 -preset veryslow -profile:v high -level 4.2
//!                            -pix_fmt yuv420p
//!                            This is also the default that is used if no options
//!                            are provided.
//! ```
//!
//! So if we have three machines c1, c2 and c3, we could do
//! ```console
//! $ shepherd -c c1,c2,c3 -l 30 source_file.mp4 output_file.mp4
//! ```
//! to have it split the video in roughly 30 second chunks and encode them in
//! parallel. By default it encodes in H.264 with a CRF value of 26 and the
//! `veryslow` preset. If you want to supply your own `ffmpeg` options for more
//! control over the codec, you can do so by adding them to the end of the
//! invocation:
//! ```console
//! $ shepherd -c c1,c2 input.mkv output.mp4 -- -c:v libx264 -crf 40
//! ```
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
//! are significantly slower than others does not delay the overall operation.
//! In the worst case, the slowest machine is the last to start encoding a
//! chunk and remains the only working encoder for the duration it takes to
//! encode this one chunk. This window can easily be reduced by using smaller
//! chunks.
//!
//! ## Performance
//!
//! As with all things parallel, Amdahl's law rears its ugly head and you don't
//! just get twice the speed with twice the processing power. With this
//! approach, you pay for having to split the video into chunks before you
//! begin, transferring them to the encoders and the results back, and
//! reassembling them. Although I should clarify that transferring the chunks
//! to the encoders only causes a noticeable delay until every encoder has its
//! first chunk, the subsequent ones can be sent while the encoders are working
//! so they don't waste time waiting for that. And returning and assembling the
//! encoded chunks doesn't carry too big of a penalty, since we're dealing with
//! much more compressed data then.
//!
//! To get a better understanding of the tradeoffs, I did some testing with a
//! couple of computers I had access to. They were my main, pretty capable
//! desktop, two older ones and a laptop. To figure out how capable each of
//! them is so we can compare the actual to the expected speedup, I let each of
//! them encode a relatively short clip of slightly less than 4 minutes taken
//! from the real video I want to encode, using the same settings I'd use for
//! the real job. And if you're wondering why encoding takes so long, it's
//! because I'm using the `veryslow` preset for maximum efficiency, even though
//! it's definitely not worth the huge increase in encoding time. But it's a
//! nice simulation for how it would look if we were using an even more
//! demanding codec like AV1.
//!
//! | machine   | duration (s) | power    |
//! | --------- | ------------ | -------- |
//! | desktop   | 1373         | 1.000    |
//! | old1      | 2571         | 0.53     |
//! | old2      | 3292         | 0.42     |
//! | laptop    | 5572         | 0.25     |
//! | **total** | -            | **2.20** |
//!
//! By giving my desktop the "power" level 1, we can determine how powerful the
//! others are at this encoding task, based on how long it takes them in
//! comparison. By adding the three other, less capable machines to the mix, we
//! slightly more than double the theoretical encoding capability of our
//! system.
//!
//! I determined these power levels on a short clip, because encoding the full
//! video would have taken very long on the less capable ones, especially the
//! laptop. But I still needed to encode the full thing on at least one of them
//! to make the comparison to the distributed encoding. I did that on my
//! desktop since it's the fastest one, and to additionally verify that the
//! power levels hold up for the full video, I bit the bullet and did the same
//! on the second most powerful machine.
//!
//! | machine | duration (s)  | power |
//! | ------- | ------------- | ----- |
//! | desktop | 9356          | 1.00  |
//! | old1    | 17690         | 0.53  |
//!
//! Now we have the baseline we want to beat with parallel encoding, as well as
//! confirmation that the power levels are valid for the full video. Let's see
//! how much of the theoretical, but unreachable 2.2x speedup we can get.
//!
//! Encoding the video in parallel took 5283 seconds, so 56.5% of the time
//! using my fastest computer, or a 1.77x speedup. We committed about twice the
//! computing power and we're not too far off that two times speedup. It's
//! making use of the additionally available resources with an 80% efficiency
//! in this case. I also tried to encode the short clip in parallel, which was
//! very fast, but had a somewhat disappointing speedup of only 1.32x. I
//! suspect that we get better results with longer videos, since encoding a
//! chunk always takes longer than creating and transferring it (otherwise
//! distributing wouldn't make sense at all). The longer the video then, the
//! larger the ratio of encoding (which we can parallelize) in the total amount
//! of time the process takes, and the more effective doing so becomes.
//!
//! I've also looked at how the work is distributed over the nodes, depending
//! on their processing power. At the end of a parallel encode, it's possible
//! to determine how many chunks have been encoded by any given host.
//!
//! | host          | chunks | power |
//! | ------------- | ------ | ----- |
//! | desktop       | 73     | 1.00  |
//! | old1          | 39     | 0.53  |
//! | old2          | 31     | 0.42  |
//! | laptop        | 19     | 0.26  |
//!
//! Inferring the processing power from the number of chunks leads to almost
//! exactly the same results as my initial determination, confirming it and
//! proving that work is distributed efficiently.
//!
//! To further see how the system scales, I've added two more machines,
//! bringing the total processing power up to 3.29.
//!
//! | machine   | duration (s) | power    |
//! | --------- | ------------ | -------- |
//! | desktop   | 1373         | 1.00     |
//! | c1        | 2129         | 0.64     |
//! | old1      | 2571         | 0.53     |
//! | c2        | 3022         | 0.45     |
//! | old2      | 3292         | 0.42     |
//! | laptop    | 5572         | 0.25     |
//! | **total** | -            | **3.29** |
//!
//! Encoding the video on these 6 machines in parallel took 3865 seconds, so
//! 41.3% of the time using my fastest computer, or a 2.42x speedup. It's
//! making use of the additionally available resources with a 74% efficiency
//! here. As expected, while we can accelerate by adding more resources, we're
//! looking at diminishing returns. Although the factor by which the efficiency
//! decreases is not as bad as it could be.
//!
//! ## Limitations
//!
//! While you can use your own `ffmpeg` options to control how the video is
//! encoded, there is currently no such option for the audio, which is 192 kb/s
//! AAC by default.

use crossbeam::channel;
use dirs;
use log::{error, info};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
    string::ToString,
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
/// * `args` - Arguments to `ffmpeg` for chunk encoding.
/// * `hosts` - Comma-separated list of hosts.
/// * `seconds` - The video chunk length.
/// * `tmp_dir` - The path to the local temporary directory.
/// * `keep` - Whether to keep temporary files on hosts (no cleanup).
pub fn run(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    args: &[&str],
    hosts: Vec<&str>,
    seconds: Option<&str>,
    tmp_dir: Option<&str>,
    keep: bool,
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
    let r = Arc::clone(&running);
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
        info!(
            "Abort signal received. Waiting for remote encoders to finish the \
             current chunk and quit gracefully."
        );
    })
    .expect("Error setting Ctrl-C handler");

    tmp_dir.push(TMP_DIR);
    // Remove local temporary directory in case it's still around
    fs::remove_dir_all(&tmp_dir).ok();
    // Create our local temporary directory
    fs::create_dir(&tmp_dir)?;

    // Start the operation
    let result = run_local(
        input.as_ref(),
        output.as_ref(),
        args,
        &tmp_dir,
        &hosts,
        seconds,
        Arc::clone(&running),
    );

    if !keep {
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
            // return unsuccessfully. With this check we prevent an error
            // message in this case, because that's what the user wants.
            // Unfortunately this also means we have to litter the `running`
            // variable almost everyhwere.
            if !output.status.success() && running.load(Ordering::SeqCst) {
                error!(
                    "Failed removing remote temporary directory on {}",
                    host
                );
            }
        }
        // Remove local temporary directory
        fs::remove_dir_all(&tmp_dir).ok();
    }

    result
}

/// Does the actual work.
///
/// This is separate so it can fail and return early, since cleanup is then
/// handled in its caller function.
fn run_local(
    input: &Path,
    output: &Path,
    args: &[&str],
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

    // Since we want to share the ffmpeg arguments between the threads, we need
    // to first set up an owned version of them
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    // and then create our Arc
    let args = Arc::new(args);

    // Create directory for encoded chunks
    let mut encoded_dir = tmp_dir.to_path_buf();
    encoded_dir.push("encoded");
    fs::create_dir(&encoded_dir)?;
    // Isolate output extension, since we want encoded chunks to have the same
    let out_ext = output
        .extension()
        .ok_or("Unable to find extension")?
        .to_str()
        .ok_or("Unable to convert OsString extension")?
        .to_string();
    // Spawn threads for hosts
    info!("Starting remote encoding");
    let mut host_threads = Vec::with_capacity(hosts.len());
    for &host in hosts {
        // Clone the queue receiver for the thread
        let thread_receiver = receiver.clone();
        // Create copy of running indicator for the thread
        let r = Arc::clone(&running);
        // And lots of other copies because it's easy and the extra allocations
        // are not a problem for this kind of application
        let host = host.to_string();
        let enc = encoded_dir.clone();
        let ext = out_ext.clone();
        let a = Arc::clone(&args);
        // Start it
        let handle =
            thread::Builder::new().name(host.clone()).spawn(|| {
                remote::host_thread(host, thread_receiver, enc, ext, a, r);
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
