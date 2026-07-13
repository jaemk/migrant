# Distribution

Docker image, cross-platform release binaries, and vendored-openssl static builds.

## DISTRI-1

A pre-built docker image is published as `jaemk/migrant:latest` (built from
`docker/Dockerfile`).

## DISTRI-2

Release binaries are built for Linux (gnu and musl), macOS (x86_64 and aarch64), and
Windows via CI workflows.

## DISTRI-3

The `vendored-openssl` cargo feature statically links OpenSSL for portable builds.

Coverage: exercised by CI build workflows, not by repo tests.
