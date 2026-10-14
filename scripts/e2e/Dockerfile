# Image to run scripts/e2e/gui_smoke.sh in, anywhere Docker runs.
#
# It carries the toolchain to build Malgel and the pieces to run its window
# headlessly: Xvfb, xdotool, ImageMagick, and Mesa's software renderers (the
# GL one is what actually paints under Xvfb; see the script's header).
#
#     docker build -f scripts/e2e/Dockerfile -t malgel-e2e .
#     docker run --rm malgel-e2e \
#       sh -c 'cargo build --release && scripts/e2e/gui_smoke.sh'
#
# To keep results, mount a volume at /out and set E2E_ARTIFACTS=/out.
FROM rust:1-bookworm

RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends \
      cmake nasm zip \
      libfontconfig-dev libwayland-dev libxkbcommon-x11-dev libx11-xcb-dev \
      libssl-dev libzstd-dev \
      fonts-dejavu-core fonts-noto-cjk \
      xvfb x11-utils x11-xserver-utils xdotool imagemagick \
      libgl1-mesa-dri libglx-mesa0 libegl1 libegl-mesa0 libgles2 libgbm1 \
      dbus-x11 \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
