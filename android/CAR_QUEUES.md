# Android Auto: playlists and per-car queues — 0.3.2

This document describes the 0.3.2 car-queue feature. See the README for the updated 0.4.0 top-level navigation.

## From the car display

MusicCraft exposes four tabs: **Now playing**, **Playlists**, **Library** and **Cars and queues**. Playlists contains dynamic mixes and saved playlists; Library contains tracks, albums and artists. When the head unit allows fewer tabs, entries are grouped while retaining content routes. Now playing shows the playlist/profile name, lists the current queue and distinguishes the selected track. The queue title is also published in the standard media session used by Android Auto. Layout and list limits depend on the head-unit system.

Open Playlists → Dynamic playlists and select a mix to start it directly from the car, without using the phone. Built-in templates and mixes previously created on the phone are available. Selection replaces only the active profile's queue. Creating/editing criteria, managing profiles and associating Bluetooth require the phone. Saved playlists start from the same menu; selecting a Now playing track retains the queue and mix generator.

## One-time phone setup

1. Open Player → **Cars and queues** (`Auto e code`), or the same entry in the options menu.
2. Choose **+ New car** (`+ Nuova auto`) and name the vehicle.
3. Open its profile, choose **Associate Bluetooth…** (`Associa Bluetooth…`) and select the head-unit device already paired in Android settings. Associate only your own car's device.
4. Enable **Bluetooth recognition** (`Riconoscimento Bluetooth`) in Cars and queues. On Android 12 and later, Nearby devices/Bluetooth connection permission is optional and must be approved by the user. Bluetooth scanning and location permissions are not requested.

On Android Auto connection, if exactly one associated device is connected, MusicCraft selects its profile. If the device is unavailable, permission is missing or recognition is ambiguous, manual selection remains available under Cars and queues on the display, or inside Library on hosts allowing few tabs. The generic Unrecognized car profile is separate from Phone and handles unidentified connections; it cannot be associated with a specific vehicle. Headphones alone are never identified as a car: an explicit association and an Android Auto connection are both required.

Manual selection takes precedence for the current connection. Disabling recognition does not delete queues. The service never changes Android's Bluetooth pairings.

## Retained data

Phone and each car retain track list/order, index/position, playlist name/origin, shuffle/repeat modes, dynamic rule and generator state. Changing profiles saves the old one and restores the destination paused; connecting never starts audio by itself. Android Auto or an explicit command can request playback. Disconnecting Android Auto restores the Phone queue paused.

Saved playlists, criteria, ratings and statistics remain shared within the installation. Separate queues do not duplicate music files. Up to twelve car profiles are supported. Missing files are excluded during restoration. Data from before 0.3.2 becomes the Phone queue without deleting playlists, criteria, ratings or Mac pairing.

Profile names and associations remain private on the phone; association stores a fingerprint of the device address, not a published device list. Nothing is sent to external services. Recognition does not require a VIN or proprietary head-unit identifier.

## Verification and limitations

`CarQueuesTest` verifies unique recognition, rejection of ambiguous/unassociated devices, consent and Android Auto requirements, and queue-link isolation. Emulator instrumentation checks Phone/Car A/Car B sessions, position, modes, playlists and mixes, Media3 controllers and legacy Android browser/session clients, starting mixes with playFromMediaId and queue titles.

Protocol tests do not replace a car or Desktop Head Unit test. They do not certify Bluetooth/Android Auto event delivery or timing on every phone/head unit, or queue layout on every display. Initial setup must be completed on the phone.

Android references:

- https://developer.android.com/training/cars/media/enable-playback
- https://developer.android.com/reference/androidx/car/app/connection/CarConnection
- https://developer.android.com/reference/android/bluetooth/BluetoothHeadset
