# Isolated cross-platform build/QA image; never mounts a user profile or audio device.
FROM --platform=linux/amd64 rust:1.97.1-bookworm@sha256:0e2bcaef56d041a486784e54104a81aebe0da44bd03019bd70bc0401e42e4a97
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    libasound2-dev libdbus-1-dev libssl-dev libudev-dev libx11-dev libxi-dev \
    libxrandr-dev libxcursor-dev libxkbcommon-dev libxkbcommon-x11-0 libwayland-dev libegl1-mesa-dev \
    libgl1-mesa-dev libglib2.0-dev pkg-config mingw-w64 nsis zip python3 \
    xvfb xauth openbox xdotool x11-utils scrot ffmpeg pulseaudio dbus-x11 \
    gnome-keyring curl ca-certificates wine64 libgl1-mesa-dri fonts-noto-core fonts-noto-cjk \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-pc-windows-gnu && rustup component add clippy rustfmt
ENV CARGO_BUILD_JOBS=2
WORKDIR /build/musiccraft
