# xmip-core-resilience-retry

Retry guard: a retryable failure below the attempt limit is tried again after a delay, flat or backing off. A technology of [xmip-core-resilience](https://github.com/IlleNilsson/xmip-core-resilience).

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
