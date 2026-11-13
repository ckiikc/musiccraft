# Synthetic decoder regression

`invalid-subframe.flac` contains a 440 Hz, 0.2 second stereo sine generated
with FFmpeg's `lavfi sine` source (44.1 kHz, FLAC). All metadata except
STREAMINFO was removed. The first frame's subframe header was changed to
`0x7e` and its FLAC CRC-16 recomputed, so the demuxer delivers an invalid
packet to the decoder. It contains no personal audio or metadata.

The regression must report the underlying decode error and must not claim
that normal shared playback failed a bit-perfect check.
