# Multiroom

The default experimental app can publish the selected source as an original-file HTTP playlist and control an **external** Lyrion Music Server. The Mac and source must remain reachable; use a private LAN address, not a loopback address, for Raspberry Pi clients. Sharing is initially off.

In Server and multiroom settings, select the source to share, start the MusicCraft media server, then connect to your LMS endpoint using the separate credential fields. Choose the destination players or group. New tracks discovered in that source update the published playlist. The player's existing queue remains under LMS control and is not silently replaced by a library update.

MusicCraft transfers original media without local playback DSP. LMS and the receiving player may choose their own decoder or transcoding path. The Mac's exclusive-output, hardware-volume and sample-rate settings do not certify the room's DAC output.

## Experimental integrated runtime for local use

The Rust integrated-server controller is present, but the default app does not contain its third-party runtime. It reports that the component is unavailable if you try to start that mode.

For an explicitly local development experiment only:

```sh
scripts/package-macos.sh release --with-lms-local
```

This downloads the pinned official macOS LMS payload, verifies its checksum and makes a local bundle. No installer is executed. The server uses an isolated profile and randomized internal credentials, is initially off, and stops with MusicCraft. Read [THIRD_PARTY.md](../THIRD_PARTY.md) before considering redistribution; the combined bundle is not an approved public release artifact.

The isolated multiroom tests use local LMS and two Squeezelite processes. They are ignored in the normal unit suite, require explicit endpoints and do not establish acoustic synchronization on physical Raspberry Pi devices.
