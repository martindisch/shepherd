# shepherd

<!-- cargo-sync-readme start -->

A distributed video encoder that splits files into chunks to encode them on
multiple machines in parallel.

## Usage

The prerequisites are one or more (you'll want more) computers—which we'll
refer to as hosts—with `ffmpeg` installed, configured such that you can SSH
into them using just their hostnames. In practice, this means you'll have
to set up your `.ssh/config` and `ssh-copy-id` your public key to the
machines. I only tested it on Linux, but if you manage to set up `ffmpeg`
and SSH, it might work on macOS or Windows directly or with little
modification.

The usage is pretty straightforward:
```text
USAGE:
    shepherd [OPTIONS] <IN> <OUT> --clients <hostnames>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -c, --clients <hostnames>    Comma-separated list of encoding hosts
    -l, --length <seconds>       The length of video chunks in seconds
    -t, --tmp <path>             The path to the local temporary directory

ARGS:
    <IN>     The original video file
    <OUT>    The output video file
```

So if we have three machines c1, c2 and c3, we could do
```console
$ shepherd -c c1,c2,c3 -l 30 source_file.mxf output_file.mp4
```
to have it split the video in roughly 30 second chunks and encode them in
parallel.

## How it works

1. Creates a temporary directory in your home directory.
2. Extracts the audio and encodes it. This is not parallelized, but the
   time this takes is negligible compared to the video anyway.
3. Splits the video into chunks. This can take relatively long, since
   you're basically writing the full file to disk again. It would be nice
   if we could read chunks of the file and directly transfer them to the
   hosts, but that might be tricky with `ffmpeg`.
4. Spawns a manager and an encoder thread for every host. The manager
   creates a temporary directory in the home directory of the remote and
   makes sure that the encoder always has something to encode. It will
   transfer a chunk, give it to the encoder to work on and meanwhile
   transfer another chunk, so the encoder can start directly with that once
   it's done, without wasting any time. But it will keep at most one chunk
   in reserve, to prevent the case where a slow machine takes too many
   chunks and is the only one still encoding while the faster ones are
   already done.
5. When an encoder is done and there are no more chunks to work on, it will
   quit and the manager transfers the encoded chunks back before
   terminating itself.
6. Once all encoded chunks have arrived, they're concatenated and the audio
   stream added.
7. All remote and the local temporary directory are removed.

Thanks to the work stealing method of distribution, having some hosts that
are significantly slower than others does not necessarily delay the overall
operation. In the worst case, the slowest machine is the last to start
encoding a chunk and remains the only working encoder for the duration it
takes to encode this one chunk. This window can easily be reduced by using
smaller chunks.

## Limitations

Currently this is tailored to my use case, which is encoding large MXF
files containing DNxHD to MP4s with H.264 and AAC streams. It's fairly easy
to change the `ffmpeg` commands to support other formats though, and in
fact making it so that the user can supply arbitrary arguments to the
`ffmpeg` processing through the CLI is a pretty low-hanging fruit and I'm
interested in that if I find the time.

As with all things parallel, Amdahl's law hits hard and you don't get twice
the speed with twice the processing power. With this approach, you pay for
having to split the video into chunks before you begin, transferring them
to the encoders and the results back, and reassembling them. But if your
system I/O (both disk and network) is good and you spend a lot of time
encoding (e.g. slow preset for H.264), it's still worth it.

<!-- cargo-sync-readme end -->

## License
Licensed under either of

 * [Apache License, Version 2.0](LICENSE-APACHE)
 * [MIT license](LICENSE-MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
