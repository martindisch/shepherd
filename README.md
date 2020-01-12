# shepherd

[![Latest version](https://img.shields.io/crates/v/shepherd)](https://crates.io/crates/shepherd)
[![Documentation](https://docs.rs/shepherd/badge.svg)](https://docs.rs/shepherd)
[![License](https://img.shields.io/crates/l/shepherd)](https://github.com/martindisch/shepherd#license)

<!-- cargo-sync-readme start -->

A distributed video encoder that splits files into chunks to encode them on
multiple machines in parallel.

## Installation

Using Cargo, you can do
```console
$ cargo install shepherd
```
or just clone the repository and compile the binary with
```console
$ git clone https://github.com/martindisch/shepherd
$ cd shepherd
$ cargo build --release
```

## Usage

The prerequisites are one or more (you'll want more) computers—which we'll
refer to as hosts—with `ffmpeg` installed and configured such that you can
SSH into them directly. This means you'll have to `ssh-copy-id` your public
key to them. I only tested it on Linux, but if you manage to set up
`ffmpeg` and SSH, it might work on macOS or Windows directly or with little
modification.

The usage is pretty straightforward:
```text
USAGE:
    shepherd [FLAGS] [OPTIONS] <IN> <OUT> --clients <hostnames>

FLAGS:
    -h, --help       Prints help information
    -k, --keep       Don't clean up temporary files on encoding hosts
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
are significantly slower than others does not delay the overall operation.
In the worst case, the slowest machine is the last to start encoding a
chunk and remains the only working encoder for the duration it takes to
encode this one chunk. This window can easily be reduced by using smaller
chunks.

## Performance

As with all things parallel, Amdahl's law rears its ugly head and you don't
just get twice the speed with twice the processing power. With this
approach, you pay for having to split the video into chunks before you
begin, transferring them to the encoders and the results back, and
reassembling them. Although I should clarify that transferring the chunks
to the encoders only causes a noticeable delay until every encoder has its
first chunk, the subsequent ones can be sent while the encoders are working
so they don't waste time waiting for that. And returning and assembling the
encoded chunks doesn't carry too big of a penalty, since we're dealing with
much more compressed data then.

To get a better understanding of the tradeoffs, I did some testing with a
couple of computers I had access to. They were my main, pretty capable
desktop, two older ones and a laptop. To figure out how capable each of
them is so we can compare the actual to the expected speedup, I let each of
them encode a relatively short clip of slightly less than 4 minutes taken
from the real video I want to encode, using the same settings I'd use for
the real job. And if you're wondering why encoding takes so long, it's
because I'm using the `veryslow` preset for maximum efficiency, even though
it's definitely not worth the huge increase in encoding time. But it's a
nice simulation for how it would look if we were using an even more
demanding codec like AV1.

| machine   | duration (s) | power    |
| --------- | ------------ | -------- |
| desktop   | 1373         | 1.000    |
| old1      | 2571         | 0.53     |
| old2      | 3292         | 0.42     |
| laptop    | 5572         | 0.25     |
| **total** | -            | **2.20** |

By giving my desktop the "power" level 1, we can determine how powerful the
others are at this encoding task, based on how long it takes them in
comparison. By adding the three other, less capable machines to the mix, we
slightly more than double the theoretical encoding capability of our
system.

I determined these power levels on a short clip, because encoding the full
video would have taken very long on the less capable ones, especially the
laptop. But I still needed to encode the full thing on at least one of them
to make the comparison to the distributed encoding. I did that on my
desktop since it's the fastest one, and to additionally verify that the
power levels hold up for the full video, I bit the bullet and did the same
on the second most powerful machine.

| machine | duration (s)  | power |
| ------- | ------------- | ----- |
| desktop | 9356          | 1.00  |
| old1    | 17690         | 0.53  |

Now we have the baseline we want to beat with parallel encoding, as well as
confirmation that the power levels are valid for the full video. Let's see
how much of the theoretical, but unreachable 2.2x speedup we can get.

Encoding the video in parallel took 5283 seconds, so 56.5% of the time
using my fastest computer, or a 1.77x speedup. We committed about twice the
computing power and we're not too far off that two times speedup. It's
making use of the additionally available resources with an 80% efficiency
in this case. I also tried to encode the short clip in parallel, which was
very fast, but had a somewhat disappointing speedup of only 1.32x. I
suspect that we get better results with longer videos, since encoding a
chunk always takes longer than creating and transferring it (otherwise
distributing wouldn't make sense at all). The longer the video then, the
larger the ratio of encoding (which we can parallelize) in the total amount
of time the process takes, and the more effective doing so becomes.

I've also looked at how the work is distributed over the nodes, depending
on their processing power. At the end of a parallel encode, it's possible
to determine how many chunks have been encoded by any given host.

| host          | chunks | power |
| ------------- | ------ | ----- |
| desktop       | 73     | 1.00  |
| old1          | 39     | 0.53  |
| old2          | 31     | 0.42  |
| laptop        | 19     | 0.26  |

Inferring the processing power from the number of chunks leads to almost
exactly the same results as my initial determination, confirming it and
proving that work is distributed efficiently.

To further see how the system scales, I've added two more machines,
bringing the total processing power up to 3.29.

| machine   | duration (s) | power    |
| --------- | ------------ | -------- |
| desktop   | 1373         | 1.00     |
| c1        | 2129         | 0.64     |
| old1      | 2571         | 0.53     |
| c2        | 3022         | 0.45     |
| old2      | 3292         | 0.42     |
| laptop    | 5572         | 0.25     |
| **total** | -            | **3.29** |

Encoding the video on these 6 machines in parallel took 3865 seconds, so
41.3% of the time using my fastest computer, or a 2.42x speedup. It's
making use of the additionally available resources with a 74% efficiency
here. As expected, while we can accelerate by adding more resources, we're
looking at diminishing returns. Although the factor by which the efficiency
decreases is not as bad as it could be.

## Limitations

Currently this is tailored to my use case, which is encoding large MXF
files containing DNxHD to MP4s with H.264 and AAC streams. It's fairly easy
to change the `ffmpeg` commands to support other formats though, and in
fact making it so that the user can supply arbitrary arguments to the
`ffmpeg` processing through the CLI is a pretty low-hanging fruit and I'm
interested in doing that if I find the time. That would be a pretty big
win, since it would allow for using this on any format that `ffmpeg`
supports and which can be losslessly split and concatenated. If you want to
use this and have problems or think about contributing, let me know by
opening an issue and I'll do my best to help.

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
