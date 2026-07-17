# Minimal image wrapping a published `migrant` CLI release binary.
#
# The Linux musl release build is fully static (its openssl is vendored), so it
# runs on a bare alpine base with only CA certificates added for TLS.
#
# Build arg `MIGRANT_VERSION` is the full CLI release tag, e.g. `cli-v1.0.0-rc.1`.
# The release workflow builds this automatically; to build by hand:
#   docker build -t migrant docker \
#     --build-arg MIGRANT_VERSION=cli-v1.0.0-rc.1
FROM alpine:3.20

ARG MIGRANT_VERSION
ARG TARGET=x86_64-unknown-linux-musl

RUN test -n "$MIGRANT_VERSION" || (echo "MIGRANT_VERSION build arg is required" && false)

RUN apk add --no-cache ca-certificates \
    && apk add --no-cache --virtual .fetch curl \
    && asset="migrant-${MIGRANT_VERSION}-${TARGET}" \
    && curl -fsSL -o /tmp/migrant.tar.gz \
        "https://github.com/jaemk/migrant/releases/download/${MIGRANT_VERSION}/${asset}.tar.gz" \
    && tar -xzf /tmp/migrant.tar.gz -C /tmp \
    && install -m 0755 "/tmp/${asset}/migrant" /usr/local/bin/migrant \
    && rm -rf /tmp/migrant.tar.gz "/tmp/${asset}" \
    && apk del .fetch \
    && migrant --help >/dev/null

# `migrant` is on PATH, so `docker run <image> migrant <subcommand>` works; a
# bare run prints help.
CMD ["migrant", "--help"]
